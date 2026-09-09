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

//! This module provides access to creating, and verifying zero-knowledge
//! proofs. It assumes that keys and IR are generated externally, which is the
//! focus of [Compact](https://github.com/input-output-hk/compactc).

use crate::curve::{Fr, outer};
use base_crypto::hash::{HashOutput, persistent_hash};
use derive_where::derive_where;
use group::ff::Field;
use group::{Group, GroupEncoding};
use lazy_static::lazy_static;
use lru::LruCache;
use midnight_circuits::types::Instantiable;
use midnight_circuits::verifier::{
    Accumulator, AssignedAccumulator, BlstrsEmulation, Msm, SelfEmulation,
};
use midnight_curves::Bls12;
use midnight_proofs::{
    poly::kzg::params::{ParamsKZG, ParamsVerifierKZG},
    utils::SerdeFormat,
};
use midnight_zk_stdlib::{MidnightVK, Relation};
#[cfg(feature = "proptest")]
use proptest::arbitrary::Arbitrary;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use rand::distributions::{Distribution, Standard};
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::Error as SerError};
use serialize::{
    Deserializable, Serializable, Tagged, VecExt, tag_enforcement_test, tagged_deserialize,
};
#[cfg(feature = "proptest")]
use serialize::{NoStrategy, simple_arbitrary};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::io::{self, Read};
#[cfg(feature = "proptest")]
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::{any::Any, cmp::Ordering};
use std::{borrow::Cow, num::NonZeroUsize};
use std::{fmt::Debug, io::Seek};
use storage_core::Storable;
use storage_core::arena::ArenaKey;
use storage_core::db::DB;
use storage_core::storable::Loader;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// A provider of prover parameters.
pub trait ParamsProverProvider {
    // Allowed because we don't care about auto traits here.
    #[allow(async_fn_in_trait)]
    /// Retrieve the parameters for a given `k` value
    async fn get_params(&self, k: u8) -> io::Result<ParamsProver>;
}

/// The hash used during proof transcript processing
pub type TranscriptHash = blake2b_simd::State;

impl ParamsProverProvider for base_crypto::data_provider::MidnightDataProvider {
    async fn get_params(&self, k: u8) -> io::Result<ParamsProver> {
        let name = Self::name_k(k);
        let reader = self
            .get_file(
                &name,
                &format!("public parameters for k={k} not found in cache"),
            )
            .await?;
        ParamsProver::read(reader)
    }
}

/// A specific instance of the prover parameters.
#[derive(Clone)]
pub struct ParamsProver(pub Arc<ParamsKZG<Bls12>>);

impl AsRef<ParamsKZG<Bls12>> for ParamsProver {
    fn as_ref(&self) -> &ParamsKZG<Bls12> {
        &self.0
    }
}

impl ParamsProver {
    /// Reads the prover parameters from a data stream
    pub fn read<R: Read>(mut reader: R) -> io::Result<Self> {
        Ok(ParamsProver(Arc::new(ParamsKZG::read_custom(
            &mut reader,
            SerdeFormat::RawBytesUnchecked,
        )?)))
    }

    pub(crate) fn as_verifier(&self) -> ParamsVerifier {
        ParamsVerifier(Arc::new(self.0.verifier_params()))
    }
}

/**
 * The maximum degree supported by the standard verifier key.
 * This limits the number of public inputs usable.
 */
pub const VERIFIER_MAX_DEGREE: u8 = 14;

/// Parameters used for verifying with the `KZG` commitment scheme
#[derive(Clone)]
pub struct ParamsVerifier(pub(crate) Arc<ParamsVerifierKZG<Bls12>>);

impl ParamsVerifier {
    /// Reads in verifier parameters
    pub fn read<R: Read>(reader: R) -> io::Result<Self> {
        Ok(ParamsProver::read(reader)?.as_verifier())
    }
}

const PARAMS_VERIFIER_RAW: &[u8] = include_bytes!("../static/bls_midnight_2p14");

lazy_static! {
    /// The midnight verifier parameters, up to [`VERIFIER_MAX_DEGREE`].
    ///
    /// Note that using this *will* embed these into the binary at compile time, if that's not what
    /// you want, please use `ParamsVerifier::read` instead.
    pub static ref PARAMS_VERIFIER: ParamsVerifier = ParamsVerifier::read(PARAMS_VERIFIER_RAW).expect("Static verifier parameters should be valid.");
}

/// A zero-knowledge proof.
///
/// `bytes` is the raw PLONK proof produced by midnight-zk. `accumulators`
/// carries one deferred KZG accumulator per `verify_proof` instruction in the
/// outer circuit, in instruction order.
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serializable, Storable)]
#[storable(base)]
#[tag = "proof[v6]"]
pub struct Proof {
    /// The raw PLONK proof bytes.
    pub bytes: Vec<u8>,
    /// Deferred KZG accumulators exposed by `verify_proof` instructions.
    pub accumulators: Vec<DeferredAccumulator>,
}
tag_enforcement_test!(Proof);

impl Proof {
    /// Builds a proof from just the PLONK bytes, with no accumulators. Use for
    /// circuits that contain no `verify_proof` instructions, and for mock or
    /// erased proofs that are never verified.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            accumulators: Vec::new(),
        }
    }
}

/// A prover key, used for creating proofs.
#[derive(Clone)]
#[derive_where(Debug; T::ProverKey)]
pub struct ProverKey<T: Zkir>(Arc<Mutex<InnerProverKey<T>>>);

/// An intermediate representation for Midnight's circuits.
#[allow(async_fn_in_trait)]
pub trait Zkir: Any + Send + Sync + Debug + Sized {
    /// The key type used for proving
    type ProverKey: Send + Sync;

    /// Check that a proof preimage satisfies the circuit
    ///
    /// Returns which outputs were skipped in the proof preimage, and how many
    /// zero element to buffer them with. Specifically, because our circuits
    /// compile to JavaScript, and there do not evaluate untaken branches, this
    /// leads to the output of the JavaScript circuit targets omitting public
    /// inputs that occurred in an untaken branch. This information still needs
    /// to be included in the statement vectors, where it is padded with zero
    /// elements.
    ///
    /// Currently, we handle this by grouping the statement vector into 'blocks'
    /// of public inputs, with each block corresponding to exactly one VM
    /// instruction, and running `check` to figure out which blocks were
    /// omitted due to untaken branches, and how many zeros to pad them with.
    ///
    /// Long-term, we probably want to move to make this obsolete, by having the
    /// computer target gather information about untaken branches at run-time.
    fn check(&self, preimage: &ProofPreimage) -> Result<Vec<Option<usize>>, ProvingError>;
    /// Proves a circuit.
    /// Returns the proof, the statement vector, and the skips from `check`.
    async fn prove(
        &self,
        rng: impl Rng + CryptoRng,
        params: &impl ParamsProverProvider,
        pk: ProverKey<Self>,
        preimage: &ProofPreimage,
    ) -> Result<(Proof, Vec<Fr>, Vec<Option<usize>>), ProvingError>;

    /// Returns the k value for this circuit
    fn k(&self) -> u8;

    /// Performs key generation on this circuit, outputting the verifier key
    async fn keygen_vk(
        &self,
        params: &impl ParamsProverProvider,
    ) -> Result<VerifierKey, anyhow::Error>;

    /// Performs key generation on this circuit, outputting the prover/verifier
    /// key pair
    async fn keygen(
        &self,
        params: &impl ParamsProverProvider,
    ) -> Result<(ProverKey<Self>, VerifierKey), anyhow::Error>;

    /// Loads IR from a tagged serialization. Separated from `Deserializable` to allow for
    /// backwards-compatible deserialization of old variants.
    fn load_ir_from_tagged(reader: impl Read + Seek) -> io::Result<Self>;

    /// Loads a prover key from a tagged serialization. Separated from `Deserializable` to allow
    /// for backwards-compatible deserialization of old variants.
    fn load_prover_key_from_tagged(reader: impl Read + Seek) -> io::Result<ProverKey<Self>>;

    /// Reads a raw (untagged) prover key from a byte stream.
    fn read_raw_pk(reader: impl Read) -> io::Result<Self::ProverKey>;
    /// Writes a raw (untagged) prover key to a byte stream.
    fn write_raw_pk(writer: impl Write, pk: &Self::ProverKey) -> io::Result<()>;
}

impl<T: Zkir> PartialEq for ProverKey<T> {
    fn eq(&self, other: &Self) -> bool {
        let mut self_ser = Vec::new();
        let mut other_ser = Vec::new();
        Serializable::serialize(self, &mut self_ser).expect("In-memory serialization must succeed");
        Serializable::serialize(other, &mut other_ser)
            .expect("In-memory serialization must succeed");
        self_ser == other_ser
    }
}

impl<T: Zkir> Eq for ProverKey<T> {}

impl<T: Zkir> Distribution<ProverKey<T>> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ProverKey<T> {
        let size: u8 = rng.gen_range(0..32);
        let mut bytes = Vec::with_bounded_capacity(size as usize);
        rng.fill_bytes(&mut bytes);
        ProverKey(Arc::new(Mutex::new(InnerProverKey::Uninitialized(bytes))))
    }
}

#[derive(Debug, Clone)]
pub(crate) enum InnerProverKey<T: Zkir> {
    Uninitialized(Vec<u8>),
    Invalid(Vec<u8>),
    Initialized(Arc<T::ProverKey>),
}

impl<T: Zkir + Tagged> Tagged for ProverKey<T> {
    fn tag() -> Cow<'static, str> {
        Cow::Owned(format!("prover-key[v7]({})", T::tag()))
    }
    fn tag_unique_factor() -> String {
        format!("prover-key[v7]({})", T::tag())
    }
}

const PK_CACHE_SIZE: usize = 5;

lazy_static! {
    // forall<T> Arc<MidnightPK<T>>
    static ref PK_CACHE: Mutex<LruCache<HashOutput, Arc<dyn Any + Send + Sync>>> =
        Mutex::new(LruCache::new(NonZeroUsize::new(PK_CACHE_SIZE).unwrap()));
}

impl<T: Zkir> InnerProverKey<T> {
    fn try_cache(&mut self) {
        let hash = match self {
            InnerProverKey::Uninitialized(data) => persistent_hash(&data[..]),
            _ => return,
        };
        if let Some(pk) = PK_CACHE
            .lock()
            .ok()
            .and_then(|mut c| c.get(&hash).cloned())
            .and_then(|ptr| ptr.downcast().ok())
        {
            *self = InnerProverKey::Initialized(pk);
        }
    }
}

impl<T: Zkir> ProverKey<T> {
    /// Constructs a `ProverKey` from an already-initialized raw inner key.
    pub fn from_raw(raw: T::ProverKey) -> Self {
        ProverKey(Arc::new(Mutex::new(InnerProverKey::Initialized(Arc::new(
            raw,
        )))))
    }

    /// Initializes the lazy prover key
    pub fn init(&self) -> Result<Arc<T::ProverKey>, ProvingError> {
        let mut mutex = self.0.lock().expect("mutex is not poisoned");
        mutex.try_cache();
        let data = match &*mutex {
            InnerProverKey::Initialized(key) => {
                return Ok(key.clone());
            }
            InnerProverKey::Invalid(_) => {
                return Err(anyhow::anyhow!("known invalid verifier key"));
            }
            InnerProverKey::Uninitialized(data) => data.clone(),
        };
        let mut inner_reader = &mut &data[..];
        let read_inner = |inner_reader| {
            let pk = T::read_raw_pk(inner_reader)?;
            Ok(pk)
        };
        let res: Result<_, ProvingError> = read_inner(&mut inner_reader);
        match res {
            Ok(pk) => {
                let key = Arc::new(pk);
                PK_CACHE
                    .lock()
                    .ok()
                    .and_then(|mut c| c.put(persistent_hash(&data), key.clone()));
                *mutex = InnerProverKey::Initialized(key.clone());
                Ok(key)
            }
            Err(e) => {
                *mutex = InnerProverKey::Invalid(data);
                Err(e)
            }
        }
    }

    fn inner_serialize<W: std::io::Write>(&self, mut writer: W) -> std::io::Result<()> {
        match &*self.0.lock().expect("mutex is not poisoned") {
            InnerProverKey::Uninitialized(data) | InnerProverKey::Invalid(data) => {
                writer.write_all(data)?;
                Ok(())
            }
            InnerProverKey::Initialized(key) => T::write_raw_pk(&mut writer, key),
        }
    }
}

struct Count(usize);

impl std::io::Write for Count {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<T: Zkir> Serializable for ProverKey<T> {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        let mut count = Count(0);
        self.inner_serialize(&mut count).ok();
        Serializable::serialize(&(count.0 as u64), writer)?;
        self.inner_serialize(writer)
    }

    fn serialized_size(&self) -> usize {
        let mut writer = Count(0);
        self.inner_serialize(&mut writer).ok();
        (writer.0 as u64).serialized_size() + writer.0
    }
}

impl<T: Zkir> Deserializable for ProverKey<T> {
    fn deserialize(reader: &mut impl Read, recursion_depth: u32) -> Result<Self, std::io::Error> {
        let buf = <Vec<u8> as Deserializable>::deserialize(reader, recursion_depth)?;
        let mut pk = InnerProverKey::Uninitialized(buf);
        pk.try_cache();
        Ok(Self(Arc::new(Mutex::new(pk))))
    }
}

/// Self-emulation used for in-circuit BLS12-381 proof verification, and for
/// reconstructing/finalizing the deferred KZG accumulators such proofs produce.
pub type InnerSelfEmulation = BlstrsEmulation;

/// Number of public-input field elements occupied by one fully-collapsed,
/// single-point-per-side accumulator: two points and two scalars, encoded as
/// field elements per the [`Instantiable`] impl of [`AssignedAccumulator`].
pub fn accumulator_pi_len() -> usize {
    <AssignedAccumulator<InnerSelfEmulation> as Instantiable<outer::Scalar>>::as_public_input(
        &Accumulator::<InnerSelfEmulation>::trivial(&[]),
    )
    .len()
}

/// Reconstructs a single-point-per-side accumulator from its public-input
/// encoding (`lhs_point || lhs_scalar || rhs_point || rhs_scalar`).
///
/// `None` on a wrong field count, or a side whose point fields do not decode to
/// a curve point in the prime-order subgroup. Does not check that the
/// accumulator is collapsed.
pub fn reconstruct_accumulator(
    fields: &[outer::Scalar],
) -> Option<Accumulator<InnerSelfEmulation>> {
    if fields.len() != accumulator_pi_len() {
        return None;
    }
    let reconstruct_side = |side: &[outer::Scalar]| -> Option<Msm<InnerSelfEmulation>> {
        let (point_fields, scalar) = side.split_at(side.len() - 1);
        let base = <<InnerSelfEmulation as SelfEmulation>::AssignedPoint as Instantiable<
            outer::Scalar,
        >>::from_public_input(point_fields)?;
        Some(Msm::new(&[base], &[scalar[0]], &BTreeMap::new()))
    };
    let half = fields.len() / 2;
    let lhs = reconstruct_side(&fields[..half])?;
    let rhs = reconstruct_side(&fields[half..])?;
    Some(Accumulator::new(lhs, rhs))
}

/// A deferred KZG accumulator, as a `verify_proof` instruction exposes it.
///
/// Only two points, because that is all one carries: an exposed accumulator has
/// been through `resolve_fixed_bases` and `collapse`, leaving each side a single
/// base whose scalar is constrained to one. The scalars are regenerated by
/// [`as_public_input`](Self::as_public_input) rather than stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeferredAccumulator {
    lhs: outer::Affine,
    rhs: outer::Affine,
}

impl DeferredAccumulator {
    /// The compressed encoding of both points.
    fn to_bytes(self) -> [u8; 2 * outer::POINT_BYTES] {
        let mut out = [0u8; 2 * outer::POINT_BYTES];
        out[..outer::POINT_BYTES].copy_from_slice(self.lhs.to_bytes().as_ref());
        out[outer::POINT_BYTES..].copy_from_slice(self.rhs.to_bytes().as_ref());
        out
    }

    /// Rebuilds the accumulator midnight-circuits works with, restoring the
    /// scalars that `collapse` had set to one.
    pub fn to_accumulator(self) -> Accumulator<InnerSelfEmulation> {
        let side =
            |p: outer::Affine| Msm::new(&[p.into()], &[outer::Scalar::ONE], &BTreeMap::new());
        Accumulator::new(side(self.lhs), side(self.rhs))
    }

    /// `None` unless `acc` is collapsed and fixed-base-resolved: one base per
    /// side, its scalar one, and no fixed-base scalars left.
    pub fn from_accumulator(acc: &Accumulator<InnerSelfEmulation>) -> Option<Self> {
        let side = |msm: Msm<InnerSelfEmulation>| -> Option<outer::Affine> {
            let (bases, scalars) = (msm.bases(), msm.scalars());
            if bases.len() != 1 || scalars.len() != 1 || scalars[0] != outer::Scalar::ONE {
                return None;
            }
            if !msm.fixed_base_scalars().is_empty() {
                return None;
            }
            Some(bases[0].into())
        };
        Some(DeferredAccumulator {
            lhs: side(acc.lhs())?,
            rhs: side(acc.rhs())?,
        })
    }

    /// The public-input encoding the outer circuit constrained, which the
    /// verifier prepends to its own statement.
    pub fn as_public_input(self) -> Vec<outer::Scalar> {
        <AssignedAccumulator<InnerSelfEmulation> as Instantiable<outer::Scalar>>::as_public_input(
            &self.to_accumulator(),
        )
    }

    /// Inverse of [`as_public_input`](Self::as_public_input).
    pub fn from_public_input(fields: &[outer::Scalar]) -> Option<Self> {
        Self::from_accumulator(&reconstruct_accumulator(fields)?)
    }
}

// Ordered and hashed by the wire encoding: the points have no ordering of their
// own, and `Proof` needs both to sit in the ledger's storage types.
impl Ord for DeferredAccumulator {
    fn cmp(&self, other: &Self) -> Ordering {
        self.to_bytes().cmp(&other.to_bytes())
    }
}

impl PartialOrd for DeferredAccumulator {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for DeferredAccumulator {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.to_bytes().hash(state)
    }
}

impl Tagged for DeferredAccumulator {
    fn tag() -> Cow<'static, str> {
        Cow::Borrowed("deferred-accumulator")
    }
    fn tag_unique_factor() -> String {
        "deferred-accumulator".into()
    }
}
tag_enforcement_test!(DeferredAccumulator);

impl Serializable for DeferredAccumulator {
    fn serialize(&self, writer: &mut impl Write) -> Result<(), io::Error> {
        writer.write_all(&self.to_bytes())
    }
    fn serialized_size(&self) -> usize {
        2 * outer::POINT_BYTES
    }
}

impl Deserializable for DeferredAccumulator {
    fn deserialize(reader: &mut impl Read, _recursion_depth: u32) -> Result<Self, io::Error> {
        let mut buf = [0u8; 2 * outer::POINT_BYTES];
        reader.read_exact(&mut buf)?;
        let point = |bytes: &[u8]| -> Result<outer::Affine, io::Error> {
            let mut repr = <outer::Affine as GroupEncoding>::Repr::default();
            repr.as_mut().copy_from_slice(bytes);
            Option::<outer::Affine>::from(outer::Affine::from_bytes(&repr)).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "accumulator point is not on the curve, or outside the prime-order subgroup",
                )
            })
        };
        Ok(DeferredAccumulator {
            lhs: point(&buf[..outer::POINT_BYTES])?,
            rhs: point(&buf[outer::POINT_BYTES..])?,
        })
    }
}

#[cfg(feature = "proptest")]
impl Arbitrary for DeferredAccumulator {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with((): Self::Parameters) -> Self::Strategy {
        use proptest::prelude::*;
        // Scaled from the generator rather than built from bytes, so every
        // value lands in the subgroup, as deserialization insists on.
        any::<(u64, u64)>()
            .prop_map(|(a, b)| {
                let g = <InnerSelfEmulation as SelfEmulation>::C::generator();
                DeferredAccumulator {
                    lhs: (g * outer::Scalar::from(a)).into(),
                    rhs: (g * outer::Scalar::from(b)).into(),
                }
            })
            .boxed()
    }
}

/// A verifier key, used for checking proofs.
#[derive(Debug, Storable)]
#[storable(base)]
pub struct VerifierKey(Arc<Mutex<InnerVerifierKey>>);

#[cfg(feature = "proptest")]
simple_arbitrary!(VerifierKey);

impl Tagged for VerifierKey {
    fn tag() -> Cow<'static, str> {
        Cow::Borrowed("verifier-key[v8]")
    }
    fn tag_unique_factor() -> String {
        "verifier-key[v8]".into()
    }
}
tag_enforcement_test!(VerifierKey);

impl Distribution<VerifierKey> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> VerifierKey {
        let size: u8 = rng.r#gen();
        let mut bytes = Vec::with_bounded_capacity(size as usize);
        rng.fill_bytes(&mut bytes);
        VerifierKey(Arc::new(Mutex::new(InnerVerifierKey::Uninitialized(bytes))))
    }
}

impl From<MidnightVK> for VerifierKey {
    fn from(vk: MidnightVK) -> Self {
        let mut raw = Vec::new();
        vk.write(&mut raw, SerdeFormat::Processed)
            .expect("in-memory serialize");
        VerifierKey(Arc::new(Mutex::new(InnerVerifierKey::Initialized(vk, raw))))
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Some features don't try to initialize
#[allow(clippy::large_enum_variant)]
pub(crate) enum InnerVerifierKey {
    Uninitialized(Vec<u8>),
    Invalid(Vec<u8>),
    /// Initialized VK with the original raw bytes preserved.
    Initialized(MidnightVK, Vec<u8>),
}

impl Clone for VerifierKey {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Deserializable for VerifierKey {
    fn deserialize(
        reader: &mut impl std::io::Read,
        recursion_depth: u32,
    ) -> Result<Self, std::io::Error> {
        const MAX_EXPECTED_SIZE: usize = 50_000;
        let buf = <Vec<u8> as Deserializable>::deserialize(reader, recursion_depth)?;
        if buf.len() > MAX_EXPECTED_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Declared vk size {} exceeded permitted limit of {MAX_EXPECTED_SIZE}",
                    buf.len()
                ),
            ));
        }
        Ok(Self(Arc::new(Mutex::new(InnerVerifierKey::Uninitialized(
            buf,
        )))))
    }
}

#[derive(Clone)]
struct DummyRelation;

// TODO: This is a temporary workaround for verifier key deserialization.
// Longer-term, we'll need to store information about the circuit architecture
// in the verifier key, and use that for deserializing, but those API endpoints
// do not currently exist in midnight-circuits.
impl Relation for DummyRelation {
    type Error = midnight_proofs::plonk::Error;
    type Instance = Vec<outer::Scalar>;
    type Witness = ();
    fn format_instance(
        instance: &Self::Instance,
    ) -> Result<Vec<outer::Scalar>, midnight_proofs::plonk::Error> {
        Ok(instance.clone())
    }
    fn circuit(
        &self,
        _std_lib: &midnight_zk_stdlib::ZkStdLib,
        _layouter: &mut impl midnight_proofs::circuit::Layouter<outer::Scalar>,
        _instance: midnight_proofs::circuit::Value<Self::Instance>,
        _witness: midnight_proofs::circuit::Value<Self::Witness>,
    ) -> Result<(), midnight_proofs::plonk::Error> {
        unimplemented!("should not attempt to execute dummy relation")
    }
    fn read_relation<R: io::Read>(_reader: &mut R) -> io::Result<Self> {
        unimplemented!("should not attempt to read dummy relation")
    }
    fn write_relation<W: io::Write>(&self, _writer: &mut W) -> io::Result<()> {
        unimplemented!("should not attempt to write dummy relation")
    }
}

impl Serialize for VerifierKey {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut vec = Vec::new();
        <VerifierKey as Serializable>::serialize(self, &mut vec).map_err(S::Error::custom)?;
        ser.serialize_bytes(&vec)
    }
}

impl<'de> Deserialize<'de> for VerifierKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes = serde_bytes::ByteBuf::deserialize(deserializer)?;
        <VerifierKey as Deserializable>::deserialize(&mut &bytes[..], 0)
            .map_err(serde::de::Error::custom)
    }
}

#[allow(clippy::derived_hash_with_manual_eq)]
impl Hash for VerifierKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut data = Vec::new();
        Serializable::serialize(&self, &mut data).ok();
        state.write(&data);
    }
}

impl Serializable for VerifierKey {
    fn serialize(&self, writer: &mut impl Write) -> Result<(), std::io::Error> {
        let mut count = Count(0);
        self.inner_serialize(&mut count).ok();
        Serializable::serialize(&(count.0 as u64), writer)?;
        self.inner_serialize(writer)
    }

    fn serialized_size(&self) -> usize {
        let mut writer = Count(0);
        self.inner_serialize(&mut writer).ok();
        (writer.0 as u64).serialized_size() + writer.0
    }
}

impl VerifierKey {
    /// Initializes the lazy verifier key
    pub fn init(&self) -> Result<(), VerifyingError> {
        self.force_init()?;
        Ok(())
    }

    // warning! This grabs the lock! Make sure to drop the result before re-running!
    #[allow(dead_code)] // Some features don't try to initialize
    pub(crate) fn force_init(&self) -> Result<MidnightVK, VerifyingError> {
        let mut mutex = self.0.lock().expect("mutex is not poisoned");
        let data = match &*mutex {
            InnerVerifierKey::Initialized(key, _) => {
                return Ok(key.clone());
            }
            InnerVerifierKey::Invalid(_) => {
                return Err(anyhow::anyhow!("known invalid verifier key"));
            }
            InnerVerifierKey::Uninitialized(data) => data.clone(),
        };
        let reader = &mut &data[..];
        let vk = MidnightVK::read(reader, SerdeFormat::Processed)
            .map_err(|_| anyhow::anyhow!("problem reading the verifier key"))?;
        *mutex = InnerVerifierKey::Initialized(vk.clone(), data);
        Ok(vk)
    }

    fn inner_serialize<W: std::io::Write>(&self, mut writer: W) -> std::io::Result<()> {
        match &*self.0.lock().expect("mutex is not poisoned") {
            InnerVerifierKey::Uninitialized(data) | InnerVerifierKey::Invalid(data) => {
                writer.write_all(data)
            }
            InnerVerifierKey::Initialized(key, _) => key.write(&mut writer, SerdeFormat::Processed),
        }
    }

    /// Returns the original raw bytes, preserved even after initialization.
    pub fn original_bytes(&self) -> Vec<u8> {
        match &*self.0.lock().expect("mutex is not poisoned") {
            InnerVerifierKey::Uninitialized(data) | InnerVerifierKey::Invalid(data) => data.clone(),
            InnerVerifierKey::Initialized(_, original) => original.clone(),
        }
    }

    /// Checks a proof against a statement.
    pub fn verify<F: Iterator<Item = Fr>>(
        &self,
        params: &ParamsVerifier,
        proof: &Proof,
        statement: F,
    ) -> Result<(), VerifyingError> {
        let vk = self.force_init()?;

        // The caller-facing `statement` covers only the outer circuit's own public
        // inputs (binding input, communications commitment, impact fields). The
        // deferred KZG accumulators exposed by `verify_proof` instructions travel
        // on the proof itself; they are prepended here to reconstruct the full
        // public-input vector the outer circuit committed to.
        let mut pi: Vec<outer::Scalar> = proof
            .accumulators
            .iter()
            .flat_map(|acc| acc.as_public_input())
            .collect();
        pi.extend(statement.map(|f| f.0));
        trace!(statement = ?pi, "verifying proof against statement");
        midnight_zk_stdlib::verify::<DummyRelation, TranscriptHash>(
            &params.0,
            &vk,
            &pi,
            None,
            &proof.bytes,
        )
        .map_err(|_| anyhow::anyhow!("Invalid outer proof"))?;

        for acc in proof.accumulators.iter() {
            if !acc.to_accumulator().check(&params.0, &BTreeMap::new()) {
                return Err(anyhow::anyhow!(
                    "inner-proof accumulator failed pairing check"
                ));
            }
        }

        Ok(())
    }

    /// Mocks the checking of a proof against a statement
    ///
    /// We do this by running a number of CPU burn cycles calculated to be approximately
    /// equivalent in time-taken to real proof verification, including the deferred
    /// pairing check that [`verify`](Self::verify) performs on each inner-proof
    /// accumulator exposed by a `verify_proof` instruction.
    #[cfg(feature = "mock-verify")]
    pub fn mock_verify<F: Iterator<Item = Fr>>(
        &self,
        proof: &Proof,
        statement: F,
    ) -> Result<(), VerifyingError> {
        let acc_len = accumulator_pi_len();
        let pi_len = statement.count() + proof.accumulators.len() * acc_len;
        crate::mock_verify::mock_verify_for(pi_len, proof.accumulators.len())
    }

    /// Checks a sequence of proofs against their corresponding statements and verifier keys
    pub fn batch_verify<
        'a,
        F: Iterator<Item = Fr>,
        V: Iterator<Item = (&'a VerifierKey, &'a Proof, F)>,
    >(
        params: &ParamsVerifier,
        parts: V,
    ) -> Result<(), VerifyingError> {
        use midnight_zk_stdlib::batch_verify;

        let mut vks = vec![];
        let mut pis = vec![];
        let mut proofs = vec![];
        let mut acc_blocks_per_proof: Vec<Vec<DeferredAccumulator>> = vec![];

        for (vk, proof, stmt) in parts.into_iter() {
            let mut pi: Vec<outer::Scalar> = proof
                .accumulators
                .iter()
                .flat_map(|acc| acc.as_public_input())
                .collect();
            pi.extend(stmt.map(|f| f.0));
            let vk = vk.force_init()?;
            vks.push(vk);
            pis.push(pi);
            proofs.push(proof.bytes.clone());
            acc_blocks_per_proof.push(proof.accumulators.clone());
        }

        batch_verify::<TranscriptHash>(&params.0, &vks, &pis, &proofs)
            .map_err(|_| anyhow::anyhow!("Invalid proof"))?;

        for accs in &acc_blocks_per_proof {
            for acc in accs.iter() {
                if !acc.to_accumulator().check(&params.0, &BTreeMap::new()) {
                    return Err(anyhow::anyhow!(
                        "inner-proof accumulator failed pairing check"
                    ));
                }
            }
        }
        Ok(())
    }

    /// Mocks the checking of a sequence of proofs against a statement
    ///
    /// This is simulated by sequentially mocking each individual verification,
    /// it doesn't currently benefit from any performance benefits one should associate
    /// with batching
    #[cfg(feature = "mock-verify")]
    pub fn mock_batch_verify<
        'a,
        F: Iterator<Item = Fr>,
        V: Iterator<Item = (&'a VerifierKey, &'a Proof, F)>,
    >(
        parts: V,
    ) -> Result<(), VerifyingError> {
        for (vk, proof, stmt) in parts {
            vk.mock_verify(proof, stmt)?;
        }
        Ok(())
    }
}

/// A hint on where keys for a circuit can be found.
///
/// Circuit keys are associated with a string name, and are resolved at proving
/// time against a hash table of provided keys.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serializable)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct KeyLocation(pub Cow<'static, str>);

impl Zeroize for KeyLocation {
    fn zeroize(&mut self) {
        if let Cow::Owned(s) = &mut self.0 {
            s.zeroize();
        }
        self.0 = Cow::Borrowed("");
    }
}

impl Tagged for KeyLocation {
    fn tag() -> Cow<'static, str> {
        Cow::Borrowed("string")
    }
    fn tag_unique_factor() -> String {
        "string".into()
    }
}

#[derive(Serializable)]
#[tag = "wrapped-ir"]
/// A container for just the IR part of [`ProofData`].
pub struct WrappedIr(pub Vec<u8>);
tag_enforcement_test!(WrappedIr);

#[derive(Clone, Serializable)]
#[tag = "proving-data"]
/// A container for the parts required for proving
pub struct ProvingKeyMaterial {
    /// The prover key
    pub prover_key: Vec<u8>,
    /// The verifier key
    pub verifier_key: Vec<u8>,
    /// The IR source
    pub ir_source: Vec<u8>,
}
tag_enforcement_test!(ProvingKeyMaterial);

/// A mechanism to retrieve / resolve zero-knowledge key material from a short location string.
pub trait Resolver {
    /// Resolves the given key to the key material it represents, if available.
    // Allowed as we do not need auto traits here
    #[allow(async_fn_in_trait)]
    async fn resolve_key(&self, key: KeyLocation) -> io::Result<Option<ProvingKeyMaterial>>;
}

/// A tool that provides proving against opaque/serialized proof preimages
/// It is assumed (though not strictly required) that this also implements
/// `Resolver` to resolve keys.
#[allow(async_fn_in_trait)]
pub trait ProvingProvider {
    /// Check the proof preimage is valid, and if so returns the pi skip sequence
    async fn check(&self, preimage: &ProofPreimage) -> Result<Vec<Option<usize>>, anyhow::Error>;
    /// Produces the proof, optionally modifying the binding input in the proof preimage first.
    async fn prove(
        self,
        preimage: &ProofPreimage,
        overwrite_binding_input: Option<Fr>,
    ) -> Result<Proof, anyhow::Error>;
    /// Creates a copy of this provider. As providers often include an RNG, this
    /// may mutate the provider itself.
    fn split(&mut self) -> Self;
    /// Retrieves the resolver underlying this proving provider.
    fn resolver(&self) -> &impl Resolver;
}

/// An inner proof handed to an `InnerProof` instruction as a witness.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serializable,
    Hash,
    Storable,
    Zeroize,
    ZeroizeOnDrop,
)]
#[storable(base)]
#[tag = "inner-proof-witness"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub enum InnerProofWitness {
    /// The inner proof's bytes.
    Direct(Vec<u8>),
}
tag_enforcement_test!(InnerProofWitness);

/// Everything necessary to produce a proof.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serializable,
    Hash,
    Storable,
    Zeroize,
    ZeroizeOnDrop,
)]
#[storable(base)]
#[tag = "proof-preimage[v2]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct ProofPreimage {
    /// The inputs to be directly handed to the IR.
    pub inputs: Vec<Fr>,
    /// A private witness vector consumed by active witness calls in the IR.
    pub private_transcript: Vec<Fr>,
    /// A public statement vector encoding statement call information in the IR.
    pub public_transcript_inputs: Vec<Fr>,
    /// A public statement vector encoding statement call results in the IR.
    pub public_transcript_outputs: Vec<Fr>,
    /// Prover-supplied inner proofs consumed positionally by `VerifyProof`
    /// instructions — one per `VerifyProof`, in instruction order.
    pub inner_proofs: Vec<InnerProofWitness>,
    /// An arbitrary input to be bound to in the proof.
    pub binding_input: Fr,
    /// The communications commitment that will be checked, and its randomness.
    /// May be [None], in which case inputs and outputs are not committed to.
    pub communications_commitment: Option<(Fr, Fr)>,
    /// Where the keys for carrying out the proving can be found.
    pub key_location: KeyLocation,
}
tag_enforcement_test!(ProofPreimage);

impl ProofPreimage {
    /// Runs witness generation and checks for correctness without generating a
    /// proof
    #[allow(unused_variables)]
    pub fn check(&self, ir: &impl Zkir) -> Result<Vec<Option<usize>>, ProvingError> {
        ir.check(self)
    }

    /// Carries out the actual proving of the proof preimage.
    #[allow(unreachable_code, unused_variables)]
    pub async fn prove<Z: Zkir>(
        &self,
        rng: impl Rng + CryptoRng,
        params: &impl ParamsProverProvider,
        resolver: &impl Resolver,
    ) -> Result<(Proof, Vec<Option<usize>>), ProvingError> {
        let proof_data = resolver
            .resolve_key(self.key_location.clone())
            .await?
            .ok_or(anyhow::Error::msg(format!(
                "failed to find proving key for '{}'",
                self.key_location.0
            )))?;
        let ir = Z::load_ir_from_tagged(io::Cursor::new(&proof_data.ir_source[..]))?;
        let verifier_key = tagged_deserialize::<VerifierKey>(&mut &proof_data.verifier_key[..])?;
        let prover_key =
            Z::load_prover_key_from_tagged(io::Cursor::new(&proof_data.prover_key[..]))?;
        let (proof, pis, pi_skips) = ir.prove(rng, params, prover_key, self).await?;
        debug!("proof created; verifying to make sure");
        let k = verifier_key.force_init()?.k();
        if let Err(e) = verifier_key.verify(
            &params.get_params(k).await?.as_verifier(),
            &proof,
            pis.iter().copied(),
        ) {
            error!(error = ?e, ?pis, ?ir, "self-verification failed! This may be a bug, check that your keys match!");
            return Err(e);
        }
        debug!("proof ok");
        Ok((proof, pi_skips))
    }
}

impl PartialEq for VerifierKey {
    fn eq(&self, other: &Self) -> bool {
        let mut self_ser = Vec::new();
        let mut other_ser = Vec::new();
        Serializable::serialize(self, &mut self_ser).expect("In-memory serialization must succeed");
        Serializable::serialize(other, &mut other_ser)
            .expect("In-memory serialization must succeed");
        self_ser == other_ser
    }
}

impl Eq for VerifierKey {}

impl PartialOrd for VerifierKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for VerifierKey {
    fn cmp(&self, other: &Self) -> Ordering {
        let mut self_ser = Vec::new();
        let mut other_ser = Vec::new();
        Serializable::serialize(self, &mut self_ser).expect("In-memory serialization must succeed");
        Serializable::serialize(other, &mut other_ser)
            .expect("In-memory serialization must succeed");
        self_ser.cmp(&other_ser)
    }
}

/// An error during proving. The type of this should not be considered part of
/// the public API, although it may be assumed to be [`Debug`]` +
/// `[`Display`](std::fmt::Display).
pub type ProvingError = anyhow::Error;
/// An error during verifying. The type of this should not be considered part of
/// the public API, although it may be assumed to be [`Debug`]` +
/// `[`Display`](std::fmt::Display).
pub type VerifyingError = anyhow::Error;

#[cfg(test)]
mod accumulator_discharge_tests {
    use super::*;
    use group::Group;

    type C = <InnerSelfEmulation as SelfEmulation>::C;

    /// A collapsed accumulator that does *not* satisfy the pairing invariant.
    fn non_pairing_accumulator() -> Accumulator<InnerSelfEmulation> {
        let one = outer::Scalar::from(1u64);
        Accumulator::new(
            Msm::new(&[C::generator()], &[one], &BTreeMap::new()),
            Msm::new(&[C::identity()], &[one], &BTreeMap::new()),
        )
    }

    fn encode(acc: &Accumulator<InnerSelfEmulation>) -> Vec<outer::Scalar> {
        <AssignedAccumulator<InnerSelfEmulation> as Instantiable<outer::Scalar>>::as_public_input(
            acc,
        )
    }

    #[test]
    fn a_carried_accumulator_that_does_not_pair_is_rejected() {
        let trivial = Accumulator::<InnerSelfEmulation>::trivial(&[]);
        assert!(
            trivial.check(&PARAMS_VERIFIER.0, &BTreeMap::new()),
            "the trivial accumulator must pair; a guarded-off instruction exposes it"
        );
        assert!(
            !non_pairing_accumulator().check(&PARAMS_VERIFIER.0, &BTreeMap::new()),
            "an accumulator whose sides differ must not pair"
        );
    }

    #[test]
    fn every_exposed_accumulator_is_checked_on_its_own() {
        let carried = [
            DeferredAccumulator::from_accumulator(&Accumulator::<InnerSelfEmulation>::trivial(&[]))
                .expect("the trivial accumulator is collapsed"),
            DeferredAccumulator::from_accumulator(&non_pairing_accumulator())
                .expect("collapsed, but does not pair"),
        ];

        let verdicts: Vec<bool> = carried
            .iter()
            .map(|acc| {
                acc.to_accumulator()
                    .check(&PARAMS_VERIFIER.0, &BTreeMap::new())
            })
            .collect();
        assert_eq!(
            verdicts,
            vec![true, false],
            "each accumulator must be checked on its own, not subsumed by the first"
        );
    }

    /// The public inputs are no longer carried, they are regenerated from the
    /// two points, so this equality is what keeps a proof verifiable.
    #[test]
    fn the_public_input_encoding_round_trips() {
        for acc in [
            Accumulator::<InnerSelfEmulation>::trivial(&[]),
            non_pairing_accumulator(),
        ] {
            let fields = encode(&acc);
            assert_eq!(fields.len(), accumulator_pi_len());

            let deferred = DeferredAccumulator::from_accumulator(&acc).expect("collapsed");
            assert_eq!(
                deferred.as_public_input(),
                fields,
                "regenerating the public inputs must reproduce what the circuit constrained"
            );
            assert_eq!(
                DeferredAccumulator::from_public_input(&fields),
                Some(deferred)
            );
        }
    }

    #[test]
    fn the_wire_encoding_round_trips() {
        let acc = DeferredAccumulator::from_accumulator(&non_pairing_accumulator()).unwrap();
        let mut bytes = Vec::new();
        acc.serialize(&mut bytes).unwrap();
        assert_eq!(bytes.len(), acc.serialized_size());
        assert_eq!(
            DeferredAccumulator::deserialize(&mut &bytes[..], 0).unwrap(),
            acc
        );
    }

    /// Derived from midnight-circuits' foreign-field encoding, and it fixes the
    /// public-input layout of every proof carrying an accumulator. A change to
    /// it is a wire-format change, so pin the value rather than discover it.
    #[test]
    fn the_accumulator_public_input_length_is_fixed() {
        assert_eq!(accumulator_pi_len(), 12);
    }

    #[test]
    fn a_malformed_encoding_is_rejected() {
        assert!(DeferredAccumulator::from_public_input(&[]).is_none());
        assert!(DeferredAccumulator::from_public_input(&[outer::Scalar::from(1u64); 3]).is_none());

        // A point off the curve must not deserialize.
        let bytes = [0xffu8; 2 * outer::POINT_BYTES];
        assert!(DeferredAccumulator::deserialize(&mut &bytes[..], 0).is_err());
    }

    /// One point, one encoding. A second encoding of the same accumulator would
    /// be a second serialization of the same proof, and so a second transaction
    /// hash, so the decoder has to refuse every non-canonical form.
    #[test]
    fn the_wire_encoding_is_canonical() {
        let repr_of = |bytes: [u8; outer::POINT_BYTES]| {
            let mut repr = <outer::Affine as GroupEncoding>::Repr::default();
            repr.as_mut().copy_from_slice(&bytes);
            repr
        };
        let compressed = |p: outer::Affine| {
            let mut out = [0u8; outer::POINT_BYTES];
            out.copy_from_slice(p.to_bytes().as_ref());
            out
        };

        // The infinity bit set over a non-zero x: the classic non-canonical form.
        let mut dirty_infinity = compressed(C::generator().into());
        dirty_infinity[0] |= 0x40;
        assert!(bool::from(
            outer::Affine::from_bytes(&repr_of(dirty_infinity)).is_none()
        ));

        // An x at or beyond the base field modulus.
        let mut oversized = [0xffu8; outer::POINT_BYTES];
        oversized[0] = 0x9f;
        assert!(bool::from(
            outer::Affine::from_bytes(&repr_of(oversized)).is_none()
        ));

        // Identity has one spelling, not a family of them.
        let identity = compressed(C::identity().into());
        assert_eq!(identity[0], 0xc0);
        assert!(identity[1..].iter().all(|b| *b == 0));
    }

    /// A non-collapsed accumulator has no `DeferredAccumulator` to be, which is
    /// what makes the two-point wire format honest.
    #[test]
    fn a_non_collapsed_accumulator_is_refused() {
        let two = outer::Scalar::from(2u64);
        let scaled = Accumulator::new(
            Msm::new(&[C::generator()], &[two], &BTreeMap::new()),
            Msm::new(&[C::generator()], &[two], &BTreeMap::new()),
        );
        assert!(DeferredAccumulator::from_accumulator(&scaled).is_none());

        let mut fixed = BTreeMap::new();
        fixed.insert("unresolved".to_owned(), outer::Scalar::ONE);
        let unresolved = Accumulator::new(
            Msm::new(&[C::generator()], &[outer::Scalar::ONE], &BTreeMap::new()),
            Msm::new(&[C::generator()], &[outer::Scalar::ONE], &fixed),
        );
        assert!(DeferredAccumulator::from_accumulator(&unresolved).is_none());
    }
}
