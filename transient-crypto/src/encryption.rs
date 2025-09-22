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

//! SNARK-friendly public key encryption.
//! Our encryption scheme is:
//! We use part of El Gamal to establish a shared secret K* (a point in the embedded curve)
//! between sender and receiver. (Receiver's PK: `g^x`, we send `g^y` to establish `K* = g^{xy}`)
//! We derive a key `K` in the main curve as `H(K*.x, K*.y)`, where H is our transient hash.
//!
//! The main message is then encrypted using the transient hash as a block cipher, in CTR
//! mode, keyed with `K`. As `K` is ephemeral, we do not use an IV, and we substitute
//! Field addition for xor.

use crate::curve::{EmbeddedFr, EmbeddedGroupAffine, embedded};
use crate::curve::{FR_BYTES, Fr};
use crate::hash::transient_hash;
use crate::repr::{FieldRepr, FromFieldRepr};
use k256::elliptic_curve::subtle::CtOption;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use rand::distributions::Standard;
use rand::prelude::Distribution;
use rand::{CryptoRng, Rng};
use serde::{
    de::{Deserialize, Deserializer},
    ser::{Error, Serialize, Serializer},
};
#[cfg(feature = "proptest")]
use serialize::randomised_serialization_test;
use serialize::{Deserializable, Serializable, Tagged, tag_enforcement_test};
use std::fmt::{self, Debug, Formatter};
use std::iter::once;
use zeroize::Zeroize;

/// A public key, consisting of a group element `g^x`
#[derive(Copy, Clone, Debug, Eq, Serializable)]
#[tag = "encryption-public-key[v1]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct PublicKey(EmbeddedGroupAffine);

impl Distribution<PublicKey> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> PublicKey {
        PublicKey(rng.r#gen())
    }
}

#[cfg(feature = "proptest")]
randomised_serialization_test!(PublicKey);
tag_enforcement_test!(PublicKey);

impl PartialEq for PublicKey {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Serialize for PublicKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut vec = Vec::new();
        <PublicKey as Serializable>::serialize(self, &mut vec).map_err(S::Error::custom)?;
        serializer.serialize_bytes(&vec)
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes = serde_bytes::ByteBuf::deserialize(deserializer)?;
        <PublicKey as Deserializable>::deserialize(&mut &bytes[..], 0)
            .map_err(serde::de::Error::custom)
    }
}

/// A secret key, the discrete logarithm of the corresponding [`PublicKey`].
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[derive(Copy, Clone, Eq, Serializable, Zeroize)]
#[tag = "encryption-secret-key[v1]"]
pub struct SecretKey(EmbeddedFr);

impl PartialEq for SecretKey {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Debug for SecretKey {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "<encryption secret key>")
    }
}

#[cfg(feature = "proptest")]
randomised_serialization_test!(SecretKey);
tag_enforcement_test!(SecretKey);

/// A ciphertext. The ciphertext includes an encryption of a zero element, which
/// is used for testing decryption.
#[derive(Clone, Debug, Serializable, PartialEq, Eq)]
pub struct Ciphertext {
    /// The challenge `g^y`.
    pub c: EmbeddedGroupAffine,
    /// The ciphertext, encrypted with `g^{xy}`.
    pub ciph: Vec<Fr>,
}

impl PublicKey {
    /// Encrypts a message that can be represented as field elements to a public key.
    pub fn encrypt<R: Rng + CryptoRng + ?Sized, T: FieldRepr>(
        &self,
        rng: &mut R,
        msg: &T,
    ) -> Ciphertext {
        let y: EmbeddedFr = rng.r#gen();
        let c = EmbeddedGroupAffine::generator() * y;
        let k_star = self.0 * y;
        let coords = if k_star.is_infinity() {
            (0.into(), 0.into())
        } else {
            (k_star.x().unwrap(), k_star.y().unwrap())
        };
        let k = transient_hash(&[coords.0, coords.1]);
        let ciph = once(0.into())
            .chain(msg.field_vec())
            .enumerate()
            .map(|(ctr, msg)| transient_hash(&[k, (ctr as u64).into()]) + msg)
            .collect();
        Ciphertext { c, ciph }
    }
}

impl SecretKey {
    /// Number of bytes needed to represent a secret key in memory
    pub const BYTES: usize = FR_BYTES;

    /// Initializes a key-pair.
    pub fn new<R: Rng + CryptoRng + ?Sized>(rng: &mut R) -> Self {
        SecretKey(rng.r#gen())
    }

    /// Initialize a key-pair from arbitrary 64 bytes (little-endian) ensuring the result falls into the space by taking modulo
    pub fn from_uniform_bytes(bytes: &[u8; 64]) -> Self {
        let value = embedded::Scalar::from_bytes_wide(bytes);
        SecretKey(EmbeddedFr(value))
    }

    /// Initialize a key-pair from repr bytes
    pub fn from_repr(bytes: &[u8; Self::BYTES]) -> CtOption<Self> {
        let val = embedded::Scalar::from_bytes(bytes);
        val.map(|scalar| SecretKey(EmbeddedFr(scalar)))
    }

    /// Converts a `SecretKey` into a raw bytes representation
    pub fn repr(&self) -> [u8; Self::BYTES] {
        self.0.0.to_bytes()
    }

    /// Derives the public key from the secret key.
    pub fn public_key(&self) -> PublicKey {
        PublicKey(EmbeddedGroupAffine::generator() * self.0)
    }

    /// Attempts decryption of a given ciphertext.
    pub fn decrypt<T: FromFieldRepr>(&self, ciph: &Ciphertext) -> Option<T> {
        let k_star = ciph.c * self.0;
        let coords = if k_star.is_infinity() {
            (0.into(), 0.into())
        } else {
            (k_star.x().unwrap(), k_star.y().unwrap())
        };
        let k = transient_hash(&[coords.0, coords.1]);
        let plain = ciph
            .ciph
            .iter()
            .enumerate()
            .map(|(ctr, ciph)| *ciph - transient_hash(&[k, (ctr as u64).into()]))
            .collect::<Vec<_>>();
        if plain.is_empty() || plain[0] != 0.into() {
            debug!("zero element check in decryption failed");
            return None;
        }
        T::from_field_repr(&plain[1..])
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "proptest")]
    use proptest::prelude::*;
    #[cfg(feature = "proptest")]
    use rand::{SeedableRng, rngs::StdRng};

    use super::*;

    #[cfg(feature = "proptest")]
    proptest! {
        #[test]
        fn correctness(
            key in <SecretKey as Arbitrary>::arbitrary(),
            msg in proptest::array::uniform32(proptest::num::u8::ANY)
        ) {
            let mut rng = StdRng::from_seed([0x42; 32]);
            let ciph = key.public_key().encrypt(&mut rng, &msg);
            let dec = key.decrypt(&ciph);
            assert_eq!(dec, Some(msg));
        }
    }

    #[test]
    fn secret_key_repr_roundtrip() {
        let seeds: Vec<[u8; 64]> = vec![[0; 64], [1; 64], [255; 64]];

        for seed in seeds {
            let key = SecretKey::from_uniform_bytes(&seed);
            let repr = key.repr();
            let from_repr = SecretKey::from_repr(&repr).unwrap();
            assert_eq!(from_repr.repr(), repr);
        }
    }
}
