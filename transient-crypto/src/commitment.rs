// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
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
use storage_core::Storable;
use storage_core::arena::ArenaKey;
use storage_core::db::DB;
use storage_core::storable::Loader;

/// Homomorphic Pedersen commitment.
/// a) Summed commitments should verify against their summed randomness.
/// b) Summed commitments should be equal to a sum of (for each type) the value sum.
#[cfg(not(feature = "unsafe-commitments-as-octets"))]
#[derive(
    Default, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serializable, Serialize, Storable,
)]
#[storable(base)]
#[tag = "pedersen[v1]"]
#[serde(transparent)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct Pedersen(pub EmbeddedGroupAffine);

/// The same commitment, kept as the octets it arrived as.
///
/// ⌖ **A representation switch, not a decoder switch — and that distinction is the whole
/// design.** The wire form is *compressed*: 32 octets carrying `v` and the sign of `u`. `y` is
/// not in there, so recovering it is a modular square root and no amount of cleverness in a
/// decoder can skip it. The only way not to pay is not to build a point.
///
/// An applier never needs one. It hashes commitments and compares them; the arithmetic that
/// makes them *binding* — summing inputs against outputs — belongs to whoever verifies. So under
/// this feature the arithmetic is **absent rather than slow**, and a build that tries to balance
/// commitments fails to compile instead of quietly decompressing two points per addition.
///
/// ⚠︎ Enable only in a binary whose commitments have already been accepted by a verifier. See
/// the guard below, and prefer [`PedersenVerified`] where a *type* can carry the distinction —
/// this exists for the places one cannot reach without churning every signature it appears in.
#[cfg(feature = "unsafe-commitments-as-octets")]
#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Storable,
)]
#[storable(base)]
#[tag = "pedersen[v1]"]
#[serde(transparent)]
pub struct Pedersen(pub [u8; 32]);

#[cfg(feature = "unsafe-commitments-as-octets")]
impl Tagged for Pedersen {
    /// The same tag as the point representation: one type on the wire, two representations of it.
    fn tag() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("pedersen[v1]")
    }

    fn tag_unique_factor() -> String {
        String::from("pedersen[v1]")
    }
}

#[cfg(feature = "unsafe-commitments-as-octets")]
impl std::fmt::Display for Pedersen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

#[cfg(feature = "unsafe-commitments-as-octets")]
impl Serializable for Pedersen {
    fn serialize(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        writer.write_all(&self.0)
    }

    fn serialized_size(&self) -> usize {
        32
    }
}

#[cfg(feature = "unsafe-commitments-as-octets")]
impl Deserializable for Pedersen {
    fn deserialize(reader: &mut impl std::io::Read, _recursion_depth: u32) -> std::io::Result<Self> {
        let mut b = [0u8; 32];
        reader.read_exact(&mut b)?;
        Ok(Pedersen(b))
    }
}

// ⚠︎ **The guard.** Cargo features are additive: anything in the graph turning this on turns it
// on for everyone. A build that fetches parameters, embeds them, or offers a CLI is not an
// applier, so its presence alongside any of those means the feature has been unified in by
// accident — which would silently remove the check that makes commitments binding.
#[cfg(all(
    feature = "unsafe-commitments-as-octets",
    any(feature = "data-provider", feature = "embed-params", feature = "cli")
))]
compile_error!(
    "`unsafe-commitments-as-octets` replaces Pedersen commitments with their unvalidated wire \
     octets and removes the arithmetic that makes them binding. It is for a binary that only \
     *applies* transactions a verifier has already accepted. It is enabled here alongside \
     `data-provider`, `embed-params` or `cli`, none of which an applier needs — so it has most \
     likely been unified in from another crate's feature set rather than chosen. Build the \
     applier with `default-features = false`."
);
#[cfg(not(feature = "unsafe-commitments-as-octets"))]
wrap_display!(Pedersen);
tag_enforcement_test!(Pedersen);

/// A ᴘᴇᴅᴇʀꜱᴇɴ commitment that a verifier has already accepted, kept in its wire form.
///
/// ⌖ **Why this exists.** Decoding a [`Pedersen`] costs a modular square root to recover the
/// y-coordinate from a compressed point — a ~255-bit exponentiation in the base field. Measured
/// on a metered interpreter that is 9,269,366 gas, and rebuilding from explicit `(x, y)` instead
/// costs the same, because it pays cofactor clearing rather than the square root. Either way it
/// is a curve operation per commitment.
///
/// An applier does not need one. `LedgerState::apply` carries the binding commitment through and
/// never reads it: across the whole apply path there is not one use of `binding_commitment`,
/// `downgrade` or `valid`. Those live in the verifier and in transaction construction. So a
/// transaction that has *already been verified* can carry its commitments as the octets they
/// arrived as, and pay for a point only if something asks for one — which, on that path, nothing
/// does.
///
/// ⚠︎ **The bytes are not validated.** This is sound only where a verifier has already accepted
/// the transaction these came from; that is the same obligation `VerifiedTransaction` documents,
/// and the reason this type is worth having rather than an unchecked constructor on
/// [`EmbeddedGroupAffine`] — there is no way to obtain one except from something already checked.
///
/// The wire form is byte-identical to [`Pedersen`], deliberately: the erased intent's encoding
/// feeds the transaction hash, so anything else would change hashes for every transaction.
#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Storable,
)]
#[storable(base)]
#[tag = "pedersen[v1]"]
#[serde(transparent)]
pub struct PedersenVerified(pub [u8; 32]);

impl Serializable for PedersenVerified {
    fn serialize(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        writer.write_all(&self.0)
    }

    fn serialized_size(&self) -> usize {
        32
    }
}

impl Deserializable for PedersenVerified {
    /// No square root, no cofactor clearing: the octets are taken as they stand.
    fn deserialize(reader: &mut impl std::io::Read, _recursion_depth: u32) -> std::io::Result<Self> {
        let mut b = [0u8; 32];
        reader.read_exact(&mut b)?;
        Ok(PedersenVerified(b))
    }
}

impl From<Pedersen> for PedersenVerified {
    fn from(p: Pedersen) -> Self {
        let mut b = Vec::with_capacity(32);
        Serializable::serialize(&p, &mut b).expect("a pedersen serializes to 32 octets");
        PedersenVerified(b.try_into().expect("32 octets"))
    }
}

impl Tagged for PedersenVerified {
    /// ⚠︎ **Deliberately the same tag as [`Pedersen`], and load-bearing.**
    ///
    /// The two are one type on the wire: refine writes a `Pedersen`, an applier reads a
    /// `PedersenVerified` from the very same octets. A distinct tag would make
    /// `tagged_deserialize` refuse those octets, and — because the binding is in the intent-hash
    /// preimage — a distinct *encoding* would move every transaction hash.
    fn tag() -> std::borrow::Cow<'static, str> {
        <Pedersen as Tagged>::tag()
    }

    fn tag_unique_factor() -> String {
        <Pedersen as Tagged>::tag_unique_factor()
    }
}

impl PedersenVerified {
    /// Materialise the point — the square root, paid here and only if asked.
    ///
    /// Fallible, because the octets were never validated: this is the moment the deferred check
    /// actually happens.
    pub fn to_pedersen(self) -> std::io::Result<Pedersen> {
        Deserializable::deserialize(&mut &self.0[..], 0)
    }
}

impl From<PedersenVerified> for Pedersen {
    /// ⚠︎ Panics on octets that are not a point.
    ///
    /// Infallible only because `PedersenDowngradeable` requires `Into<Pedersen>`. It is sound
    /// under this type's contract — the octets came from a transaction a verifier accepted, and
    /// a verifier cannot accept a commitment that is not a point. Where that assurance is
    /// missing, use [`PedersenVerified::to_pedersen`] and handle the error.
    fn from(v: PedersenVerified) -> Self {
        v.to_pedersen()
            .expect("a verifier accepted these octets as a commitment")
    }
}

/// The randomness used in the Pedersen commitments is the embedded curves prime
/// field.
pub type PedersenRandomness = EmbeddedFr;

#[cfg(not(feature = "unsafe-commitments-as-octets"))]
impl From<PedersenRandomness> for Pedersen {
    fn from(rand: PedersenRandomness) -> Pedersen {
        Pedersen(EmbeddedGroupAffine::generator() * rand)
    }
}

#[cfg(not(feature = "unsafe-commitments-as-octets"))]
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

#[cfg(not(feature = "unsafe-commitments-as-octets"))]
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

#[cfg(not(feature = "unsafe-commitments-as-octets"))]
impl Add<Pedersen> for Pedersen {
    type Output = Pedersen;
    fn add(self, other: Self) -> Self {
        Pedersen(self.0 + other.0)
    }
}

#[cfg(not(feature = "unsafe-commitments-as-octets"))]
impl Neg for Pedersen {
    type Output = Pedersen;
    fn neg(self) -> Self {
        Pedersen(-self.0)
    }
}

#[cfg(not(feature = "unsafe-commitments-as-octets"))]
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
#[cfg(not(feature = "unsafe-commitments-as-octets"))]
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

#[cfg(not(feature = "unsafe-commitments-as-octets"))]
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

#[cfg(test)]
mod pedersen_verified_tests {
    use super::*;
    use serialize::{Deserializable, Serializable};

    /// The property the type rests on: its wire form is byte-identical to [`Pedersen`]'s.
    ///
    /// This is not cosmetic. The erased intent's encoding feeds `to_hash_data`, and so the
    /// transaction hash — anything but byte-identical would change the hash of every
    /// transaction, which is a consensus break wearing an optimisation's clothes.
    ///
    /// The round trip also has to be exact: what a verifier accepted must come back out as the
    /// same point, or an applier is applying something else.
    #[test]
    fn the_wire_form_is_identical_and_the_round_trip_exact() {
        for seed in [1u64, 7, 1 << 40] {
            let p = Pedersen::from(PedersenRandomness::from(seed));

            let mut a = Vec::new();
            Serializable::serialize(&p, &mut a).expect("pedersen");
            let v = PedersenVerified::from(p);
            let mut b = Vec::new();
            Serializable::serialize(&v, &mut b).expect("verified");
            assert_eq!(a, b, "the wire form must not move — it feeds the tx hash");
            assert_eq!(
                <PedersenVerified as Tagged>::tag(),
                <Pedersen as Tagged>::tag(),
                "one type on the wire: a different tag makes tagged_deserialize refuse these octets"
            );
            assert_eq!(a.len(), 32);
            assert_eq!(v.serialized_size(), p.serialized_size());

            // Decoding the verified form takes the octets as they stand...
            let w: PedersenVerified =
                Deserializable::deserialize(&mut &a[..], 0).expect("verified decode");
            assert_eq!(w, v);
            // ...and materialising the point, if anyone asks, gives back what went in.
            let q = w.to_pedersen().expect("a verifier accepted this");
            assert_eq!(q, p, "the round trip must be exact, not merely close");
        }
    }

    /// ⚠︎ The type asserts nothing about its octets. Garbage decodes happily and only fails when
    /// a point is actually demanded — which is the whole point, and the whole risk. Stated as a
    /// test so the contract is executable rather than a paragraph someone skims.
    #[test]
    fn nonsense_decodes_and_only_fails_when_a_point_is_demanded() {
        let junk = [0xffu8; 32];
        let v: PedersenVerified =
            Deserializable::deserialize(&mut &junk[..], 0).expect("no validation on decode");
        assert!(
            v.to_pedersen().is_err(),
            "materialising it must fail — the check is deferred, not deleted"
        );
    }
}

#[cfg(test)]
mod commitment_representation_tests {
    use super::*;
    use serialize::Serializable;

    /// Whichever representation is compiled, the wire form must be the same 32 octets under the
    /// same tag — the two are one type on the wire, and the encoding reaches transaction hashes.
    ///
    /// The two representations cannot coexist in one build, so this runs in both and asserts the
    /// invariant each can see. Cross-checking the actual octets is `PedersenVerified`'s job,
    /// which *can* sit beside a point.
    #[test]
    fn the_wire_form_is_32_octets_under_pedersens_tag() {
        assert_eq!(<Pedersen as Tagged>::tag(), "pedersen[v1]");
        let p = Pedersen::default();
        let mut b = Vec::new();
        Serializable::serialize(&p, &mut b).expect("pedersen");
        assert_eq!(b.len(), 32, "the wire form must not change with representation");
        assert_eq!(p.serialized_size(), 32);
    }
}
