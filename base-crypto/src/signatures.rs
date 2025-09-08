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

//! Signature scheme for use primarily outside of proofs
//!
//! Schnorr over secp256k1, conforming to BIP340.
use crate::BinaryHashRepr;
use k256::schnorr;
#[cfg(feature = "proptest")]
use proptest::arbitrary::Arbitrary;
use rand::distributions::{Distribution, Standard};
use rand::rngs::OsRng;
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Serialize};
use serialize::{Deserializable, Serializable, Tagged, VecExt, tag_enforcement_test};
#[cfg(feature = "proptest")]
use serialize::{NoStrategy, simple_arbitrary};
use signature::{RandomizedSigner, Verifier};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt::{self, Debug, Formatter};
use std::hash::Hash;
use std::io::{self, Read, Write};
#[cfg(feature = "proptest")]
use std::marker::PhantomData;

macro_rules! derive_via_to_bytes {
    ($ty:ty) => {
        impl Hash for $ty {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                state.write(&self.0.to_bytes()[..]);
            }
        }

        impl PartialOrd for $ty {
            fn partial_cmp(&self, other: &$ty) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for $ty {
            fn cmp(&self, other: &$ty) -> Ordering {
                let left = self.0.to_bytes();
                let right = other.0.to_bytes();
                left.cmp(&right)
            }
        }
    };
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
/// A verifying public key
pub struct VerifyingKey(schnorr::VerifyingKey);
derive_via_to_bytes!(VerifyingKey);

impl Default for VerifyingKey {
    fn default() -> Self {
        // Manually sampled, we want a stand-in without an rng sometimes.
        VerifyingKey(
            schnorr::VerifyingKey::from_bytes(&[
                43, 59, 242, 191, 89, 80, 243, 46, 116, 47, 12, 103, 140, 35, 90, 207, 180, 68,
                188, 10, 108, 126, 200, 195, 239, 14, 120, 114, 89, 188, 199, 38,
            ])
            .expect("static verifier key should be valid"),
        )
    }
}

impl Debug for VerifyingKey {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "<signature verifying key>")
    }
}

impl BinaryHashRepr for VerifyingKey {
    fn binary_repr<W: crate::MemWrite<u8>>(&self, writer: &mut W) {
        writer.write(&self.0.to_bytes());
    }

    fn binary_len(&self) -> usize {
        self.0.to_bytes().len()
    }
}

#[cfg(feature = "proptest")]
simple_arbitrary!(VerifyingKey);
#[cfg(feature = "proptest")]
serialize::randomised_serialization_test!(VerifyingKey);

impl Distribution<VerifyingKey> for Standard {
    fn sample<R: Rng + ?Sized>(&self, _rng: &mut R) -> VerifyingKey {
        SigningKey::sample(OsRng).verifying_key()
    }
}

impl Tagged for VerifyingKey {
    fn tag() -> Cow<'static, str> {
        Cow::Borrowed("signature-verifying-key[v1]")
    }
    fn tag_unique_factor() -> String {
        "signature-verifying-key[v1]".into()
    }
}
tag_enforcement_test!(VerifyingKey);

impl Serializable for VerifyingKey {
    fn serialize(&self, writer: &mut impl Write) -> io::Result<()> {
        writer.write_all(&self.0.to_bytes())
    }

    fn serialized_size(&self) -> usize {
        // Key size is 32 (k256::Secp256k1::FieldBytesSize). Accessing this
        // would require an additional import for the trait.
        // Note that this is *field* size, because BIP340 encodes curve points as a field
        32
    }
}

impl Deserializable for VerifyingKey {
    fn deserialize(reader: &mut impl Read, _recursion_depth: u32) -> io::Result<Self> {
        let mut bytes = [0u8; 32];
        reader.read_exact(&mut bytes)?;
        Ok(VerifyingKey(
            schnorr::VerifyingKey::from_bytes(&bytes).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Malformed Schnorr verifying key",
                )
            })?,
        ))
    }
}

impl VerifyingKey {
    /// Verifies if a signature is correct
    pub fn verify(&self, msg: &[u8], signature: &Signature) -> bool {
        matches!(self.0.verify(msg, &signature.0), Ok(()))
    }
}

#[derive(Clone)]
/// A signing secret key
pub struct SigningKey(schnorr::SigningKey);

impl Tagged for SigningKey {
    fn tag() -> Cow<'static, str> {
        Cow::Borrowed("signing-key[v1]")
    }
    fn tag_unique_factor() -> String {
        "signing-key[v1]".into()
    }
}
tag_enforcement_test!(SigningKey);

impl SigningKey {
    /// Samples a new secret key from secure randomness
    pub fn sample<R: Rng + CryptoRng>(mut rng: R) -> Self {
        SigningKey(schnorr::SigningKey::random(&mut rng))
    }

    /// Returns the corresponding verifying public key
    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey(*self.0.verifying_key())
    }

    /// Signs a message
    pub fn sign<R: Rng + CryptoRng>(&self, rng: &mut R, msg: &[u8]) -> Signature {
        Signature(self.0.sign_with_rng(rng, msg))
    }

    /// Parse signing key from big endian-encoded bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let signing_key = schnorr::SigningKey::from_bytes(bytes)?;
        Ok(SigningKey(signing_key))
    }
}

impl Debug for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<secret key>")
    }
}

impl Serializable for SigningKey {
    fn serialize(&self, writer: &mut impl Write) -> io::Result<()> {
        writer.write_all(&self.0.to_bytes())
    }

    fn serialized_size(&self) -> usize {
        // Key size is 32 (k256::Secp256k1::FieldBytesSize). Accessing this
        // would require an additional import for the trait.
        32
    }
}

impl Deserializable for SigningKey {
    fn deserialize(reader: &mut impl Read, _recursion_depth: u32) -> io::Result<Self> {
        let mut bytes = [0u8; 32];
        reader.read_exact(&mut bytes)?;
        Ok(SigningKey(
            schnorr::SigningKey::from_bytes(&bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "Malformed Schnorr signing key")
            })?,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A Schnorr signature
pub struct Signature(schnorr::Signature);
derive_via_to_bytes!(Signature);

impl Default for Signature {
    fn default() -> Signature {
        // Manually sampled, we want a stand-in without an rng sometimes.
        Signature(
            schnorr::Signature::try_from(
                &[
                    20, 137, 89, 240, 159, 41, 72, 199, 212, 53, 117, 4, 235, 179, 101, 207, 210,
                    224, 132, 10, 131, 224, 89, 19, 152, 194, 235, 130, 162, 57, 186, 40, 103, 85,
                    94, 192, 157, 17, 70, 102, 209, 27, 62, 153, 67, 246, 158, 17, 124, 18, 63,
                    245, 208, 254, 72, 95, 157, 235, 180, 156, 164, 66, 143, 251,
                ][..],
            )
            .expect("static signature should be valid"),
        )
    }
}

#[cfg(feature = "proptest")]
simple_arbitrary!(Signature);
#[cfg(feature = "proptest")]
serialize::randomised_serialization_test!(Signature);

impl Distribution<Signature> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Signature {
        let signing_key = SigningKey::sample(OsRng);
        let mut message = Vec::with_bounded_capacity(32);
        rng.fill_bytes(&mut message);
        signing_key.sign(&mut OsRng, &message)
    }
}

impl Tagged for Signature {
    fn tag() -> Cow<'static, str> {
        Cow::Borrowed("signature[v1]")
    }
    fn tag_unique_factor() -> String {
        "signature[v1]".into()
    }
}
tag_enforcement_test!(Signature);

impl Serializable for Signature {
    fn serialize(&self, writer: &mut impl Write) -> io::Result<()> {
        writer.write_all(&self.0.to_bytes())
    }

    fn serialized_size(&self) -> usize {
        schnorr::Signature::BYTE_SIZE
    }
}

impl Deserializable for Signature {
    fn deserialize(reader: &mut impl Read, _recursion_depth: u32) -> io::Result<Self> {
        let mut bytes = [0u8; 64];
        reader.read_exact(&mut bytes)?;
        Ok(Signature(
            schnorr::Signature::try_from(&bytes[..]).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "Malformed Schnorr signature")
            })?,
        ))
    }
}
