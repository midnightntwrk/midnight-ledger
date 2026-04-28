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

//! ECDSA signature scheme over secp256k1.
use crate::BinaryHashRepr;
use k256::ecdsa;
#[cfg(feature = "proptest")]
use proptest::arbitrary::Arbitrary;
use rand::distributions::{Distribution, Standard};
use rand::rngs::OsRng;
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Serialize};
use serialize::{Deserializable, Serializable, Tagged, VecExt, tag_enforcement_test};
#[cfg(feature = "proptest")]
use serialize::{NoStrategy, simple_arbitrary};
use signature::{Signer, Verifier};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt::{self, Debug, Formatter};
use std::hash::Hash;
use std::io::{self, Read, Write};
#[cfg(feature = "proptest")]
use std::marker::PhantomData;

#[derive(Clone, PartialEq, Eq)]
/// A verifying public key
pub struct VerifyingKey(ecdsa::VerifyingKey);

// ecdsa::VerifyingKey does not carry serde impls (unlike schnorr::VerifyingKey),
// so we implement them manually via the 33-byte SEC1 compressed encoding.
impl Serialize for VerifyingKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self.0.to_encoded_point(true).as_bytes())
    }
}

impl<'de> Deserialize<'de> for VerifyingKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let buf = serde_bytes::ByteBuf::deserialize(deserializer)?;
        ecdsa::VerifyingKey::from_sec1_bytes(buf.as_ref())
            .map(VerifyingKey)
            .map_err(serde::de::Error::custom)
    }
}

impl Hash for VerifyingKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(self.0.to_encoded_point(true).as_bytes());
    }
}

impl PartialOrd for VerifyingKey {
    fn partial_cmp(&self, other: &VerifyingKey) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for VerifyingKey {
    fn cmp(&self, other: &VerifyingKey) -> Ordering {
        let left = self.0.to_encoded_point(true);
        let right = other.0.to_encoded_point(true);
        left.as_bytes().cmp(right.as_bytes())
    }
}

impl Default for VerifyingKey {
    fn default() -> Self {
        // Generator point.
        VerifyingKey(
            ecdsa::VerifyingKey::from_sec1_bytes(&[
                2, 121, 190, 102, 126, 249, 220, 187, 172, 85, 160, 98, 149, 206, 135, 11, 7, 2,
                155, 252, 219, 45, 206, 40, 217, 89, 242, 129, 91, 22, 248, 23, 152,
            ])
            .expect("static verifying key should be valid"),
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
        writer.write(self.0.to_encoded_point(true).as_bytes());
    }

    fn binary_len(&self) -> usize {
        // Compressed SEC1.
        33
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
        Cow::Borrowed("ecdsa-verifying-key[v1]")
    }
    fn tag_unique_factor() -> String {
        "ecdsa-verifying-key[v1]".into()
    }
}
tag_enforcement_test!(VerifyingKey);

impl Serializable for VerifyingKey {
    fn serialize(&self, writer: &mut impl Write) -> io::Result<()> {
        writer.write_all(self.0.to_encoded_point(true).as_bytes())
    }

    fn serialized_size(&self) -> usize {
        // Compressed SEC1.
        33
    }
}

impl Deserializable for VerifyingKey {
    fn deserialize(reader: &mut impl Read, _recursion_depth: u32) -> io::Result<Self> {
        let mut bytes = [0u8; 33];
        reader.read_exact(&mut bytes)?;
        Ok(VerifyingKey(
            ecdsa::VerifyingKey::from_sec1_bytes(&bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "Malformed ECDSA verifying key")
            })?,
        ))
    }
}

impl VerifyingKey {
    /// Verifies if a signature is correct.
    pub fn verify(&self, msg: &[u8], signature: &Signature) -> bool {
        matches!(Verifier::verify(&self.0, msg, &signature.0), Ok(()))
    }
}

#[derive(Clone)]
/// A signing secret key.
pub struct SigningKey(ecdsa::SigningKey);

impl Tagged for SigningKey {
    fn tag() -> Cow<'static, str> {
        Cow::Borrowed("ecdsa-signing-key[v1]")
    }
    fn tag_unique_factor() -> String {
        "ecdsa-signing-key[v1]".into()
    }
}
tag_enforcement_test!(SigningKey);

impl SigningKey {
    /// Samples a new secret key from secure randomness.
    pub fn sample<R: Rng + CryptoRng>(mut rng: R) -> Self {
        SigningKey(ecdsa::SigningKey::random(&mut rng))
    }

    /// Returns the corresponding verifying public key.
    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey(*self.0.verifying_key())
    }

    /// Signs a message deterministically (RFC 6979); no RNG is required.
    pub fn sign(&self, msg: &[u8]) -> Signature {
        Signature(Signer::sign(&self.0, msg))
    }

    /// Parse signing key from big endian-encoded bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let signing_key = ecdsa::SigningKey::from_slice(bytes)?;
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
        // Key size is 32.
        32
    }
}

impl Deserializable for SigningKey {
    fn deserialize(reader: &mut impl Read, _recursion_depth: u32) -> io::Result<Self> {
        let mut bytes = [0u8; 32];
        reader.read_exact(&mut bytes)?;
        Ok(SigningKey(
            ecdsa::SigningKey::from_slice(&bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "Malformed ECDSA signing key")
            })?,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// An ECDSA signature.
pub struct Signature(ecdsa::Signature);

impl Hash for Signature {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(&self.0.to_bytes()[..]);
    }
}

impl PartialOrd for Signature {
    fn partial_cmp(&self, other: &Signature) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Signature {
    fn cmp(&self, other: &Signature) -> Ordering {
        let left = self.0.to_bytes();
        let right = other.0.to_bytes();
        left.cmp(&right)
    }
}

impl Default for Signature {
    fn default() -> Signature {
        // (1, 1)
        Signature(
            ecdsa::Signature::from_slice(&[
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
            ])
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
        signing_key.sign(&message)
    }
}

impl Tagged for Signature {
    fn tag() -> Cow<'static, str> {
        Cow::Borrowed("ecdsa-signature[v1]")
    }
    fn tag_unique_factor() -> String {
        "ecdsa-signature[v1]".into()
    }
}
tag_enforcement_test!(Signature);

impl Serializable for Signature {
    fn serialize(&self, writer: &mut impl Write) -> io::Result<()> {
        writer.write_all(&self.0.to_bytes())
    }

    fn serialized_size(&self) -> usize {
        // 32-byte r + 32-byte s.
        64
    }
}

impl Deserializable for Signature {
    fn deserialize(reader: &mut impl Read, _recursion_depth: u32) -> io::Result<Self> {
        let mut bytes = [0u8; 64];
        reader.read_exact(&mut bytes)?;
        Ok(Signature(
            ecdsa::Signature::from_slice(&bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "Malformed ECDSA signature")
            })?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn sign_and_verify_roundtrip() {
        let sk = SigningKey::sample(OsRng);
        let vk = sk.verifying_key();
        let msg = b"hello, midnight";
        let sig = sk.sign(msg);
        assert!(vk.verify(msg, &sig));
    }

    #[test]
    fn wrong_message_does_not_verify() {
        let sk = SigningKey::sample(OsRng);
        let vk = sk.verifying_key();
        let sig = sk.sign(b"correct message");
        assert!(!vk.verify(b"wrong message", &sig));
    }

    #[test]
    fn wrong_key_does_not_verify() {
        let sk1 = SigningKey::sample(OsRng);
        let sk2 = SigningKey::sample(OsRng);
        let sig = sk1.sign(b"message");
        assert!(!sk2.verifying_key().verify(b"message", &sig));
    }

    #[test]
    fn signing_is_deterministic() {
        let sk = SigningKey::sample(OsRng);
        let msg = b"determinism check";
        assert_eq!(sk.sign(msg), sk.sign(msg));
    }

    #[test]
    fn verifying_key_serialization_roundtrip() {
        let sk = SigningKey::sample(OsRng);
        let vk = sk.verifying_key();
        let mut buf = Vec::new();
        Serializable::serialize(&vk, &mut buf).unwrap();
        assert_eq!(buf.len(), 33);
        let vk2 = <VerifyingKey as Deserializable>::deserialize(&mut buf.as_slice(), 0).unwrap();
        assert_eq!(vk, vk2);
    }

    #[test]
    fn signing_key_serialization_roundtrip() {
        let sk = SigningKey::sample(OsRng);
        let mut buf = Vec::new();
        Serializable::serialize(&sk, &mut buf).unwrap();
        assert_eq!(buf.len(), 32);
        let sk2 = <SigningKey as Deserializable>::deserialize(&mut buf.as_slice(), 0).unwrap();
        assert_eq!(sk.verifying_key(), sk2.verifying_key());
    }

    #[test]
    fn signature_serialization_roundtrip() {
        let sk = SigningKey::sample(OsRng);
        let sig = sk.sign(b"test");
        let mut buf = Vec::new();
        Serializable::serialize(&sig, &mut buf).unwrap();
        assert_eq!(buf.len(), 64);
        let sig2 = <Signature as Deserializable>::deserialize(&mut buf.as_slice(), 0).unwrap();
        assert_eq!(sig, sig2);
    }

    #[test]
    fn from_bytes_roundtrip() {
        let sk = SigningKey::sample(OsRng);
        let mut buf = Vec::new();
        Serializable::serialize(&sk, &mut buf).unwrap();
        let sk2 = SigningKey::from_bytes(&buf).unwrap();
        assert_eq!(sk.verifying_key(), sk2.verifying_key());
    }
}
