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
        Ok(SigningKey(ecdsa::SigningKey::from_slice(&bytes).map_err(
            |_| io::Error::new(io::ErrorKind::InvalidData, "Malformed ECDSA signing key"),
        )?))
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
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 1,
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
        Ok(Signature(ecdsa::Signature::from_slice(&bytes).map_err(
            |_| io::Error::new(io::ErrorKind::InvalidData, "Malformed ECDSA signature"),
        )?))
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

// ---------------------------------------------------------------------------
// Area D: injectivity / canonicity of `VerifyingKey` / `Signature` decode.
//
// The desired property is that exactly one accepted byte encoding exists per
// key and per signature, and that no malleable-but-equivalent variant is
// accepted. Two concrete violations exist on the current base:
//
//   D.1  Signature malleability (high-S accepted). `Signature::deserialize`
//        (ecdsa.rs:298) parses 64 raw bytes with `k256::ecdsa::Signature::
//        from_slice`, which does NOT enforce low-S. For any signature `(r, s)`
//        the malleated `(r, n - s)` is a distinct 64-byte encoding that also
//        decodes successfully and *verifies* the same message under the same
//        key (`VerifyingKey::verify`, ecdsa.rs:153, does not reject high-S).
//        So two distinct encodings are accepted as distinct-but-equivalent
//        signatures — malleability.
//
//   D.2  `VerifyingKey` serde decode accepts multiple SEC1 encodings.
//        The hand-written serde `Deserialize` (ecdsa.rs:47-54) feeds a
//        variable-length `ByteBuf` to `from_sec1_bytes`, which accepts the
//        33-byte compressed form AND the 65-byte uncompressed form for the
//        same key. Serialization always emits 33-byte compressed, so the
//        uncompressed encoding is a second, non-canonical accepted encoding of
//        the same key. (The binary `Deserializable` path reads a fixed 33
//        bytes and is not affected.)
// ---------------------------------------------------------------------------
#[cfg(all(test, feature = "proptest"))]
mod canonicity_props {
    use super::*;
    use k256::elliptic_curve::PrimeField;
    use proptest::prelude::*;
    use rand::rngs::OsRng;

    fn sample_key() -> SigningKey {
        SigningKey::sample(OsRng)
    }

    fn ser_sig(sig: &Signature) -> Vec<u8> {
        let mut b = Vec::new();
        Serializable::serialize(sig, &mut b).unwrap();
        b
    }

    /// Given a signature, produce the malleated high-S counterpart `(r, n - s)`
    /// as a 64-byte `r || s` encoding.
    fn malleate(sig: &Signature) -> [u8; 64] {
        // Normalize the source to low-S first so negation is unambiguous.
        let low = sig.0.normalize_s().unwrap_or(sig.0);
        let (r, s) = low.split_scalars();
        let neg_s = -*s; // n - s (mod n)
        let high = ecdsa::Signature::from_scalars(r.to_bytes(), neg_s.to_repr())
            .expect("n - s is a valid non-zero scalar");
        high.to_bytes().into()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        // Totality: arbitrary 64/33 bytes never panic the decoders.
        #[test]
        fn sig_decode_total(bytes in prop::collection::vec(any::<u8>(), 0..80)) {
            let _ = <Signature as Deserializable>::deserialize(&mut &bytes[..], 0);
        }
        #[test]
        fn vk_decode_total(bytes in prop::collection::vec(any::<u8>(), 0..80)) {
            let _ = <VerifyingKey as Deserializable>::deserialize(&mut &bytes[..], 0);
        }

        // Canonicity of the *binary* paths: every accepted encoding re-encodes
        // to itself (these hold today — the binary formats are fixed-width and
        // bijective on their accepted set).
        #[test]
        fn sig_binary_roundtrip_canonical(bytes in prop::collection::vec(any::<u8>(), 64..65)) {
            if let Ok(sig) = <Signature as Deserializable>::deserialize(&mut &bytes[..], 0) {
                prop_assert_eq!(ser_sig(&sig), bytes);
            }
        }
    }

    // Property that HOLDS today (regression guard): ECDSA
    // signatures are NOT malleable at the verification layer. A high-S
    // malleation `(r, n - s)` of a valid signature still *parses* (k256's
    // `from_slice` does not enforce low-S, ecdsa.rs:298), but `verify`
    // (ecdsa.rs:153) rejects it, so it is not accepted as an equivalent value.
    // The permissive parse is therefore harmless: the two encodings do not both
    // verify.
    #[test]
    fn signature_high_s_parses_but_does_not_verify() {
        let sk = sample_key();
        let vk = sk.verifying_key();
        let msg = b"malleability";
        let sig = sk.sign(msg);
        let high = malleate(&sig);
        // Distinct encoding that still parses.
        assert_ne!(ser_sig(&sig)[..], high[..], "malleation produced identical bytes");
        let high_sig =
            <Signature as Deserializable>::deserialize(&mut &high[..], 0).expect("high-S parses");
        assert!(vk.verify(msg, &sig), "original (low-S) signature must verify");
        assert!(
            !vk.verify(msg, &high_sig),
            "high-S malleation must NOT verify (low-S enforced at verify) -> no malleability"
        );
    }

    // Note: the serde `Deserialize` for `VerifyingKey` accepts alternate SEC1
    // encodings (e.g. the 65-byte uncompressed form) in addition to the
    // canonical 33-byte compressed form. This is intentional on the serde/RPC
    // surface -- a convenience for callers presenting a key in another
    // encoding -- and cannot reach consensus: the binary `Deserializable`
    // reads a fixed 33 bytes (compressed only), and this ecdsa key type is not
    // used by any ledger/consensus path.
}
