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

//! This module provides access to creating, and verifying zero-knowledge
//! proofs. It assumes that keys and IR are generated externally, which is the
//! focus of [Compact](https://github.com/input-output-hk/compactc).

use crate::curve::{Fr, outer};
use base_crypto::hash::{HashOutput, persistent_hash};
use lazy_static::lazy_static;
use lru::LruCache;
use midnight_circuits::compact_std_lib::{MidnightCircuit, MidnightPK, MidnightVK, Relation};
use midnight_curves::Bls12;
use midnight_proofs::{
    poly::kzg::params::{ParamsKZG, ParamsVerifierKZG},
    utils::SerdeFormat,
};
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
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::io::{self, Read};
#[cfg(feature = "proptest")]
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::{any::Any, cmp::Ordering};
use std::{borrow::Cow, num::NonZeroUsize};
use storage::Storable;
use storage::arena::ArenaKey;
use storage::db::DB;
use storage::storable::Loader;

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
pub struct ParamsProver(Arc<ParamsKZG<Bls12>>);

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
pub struct ParamsVerifier(Arc<ParamsVerifierKZG<Bls12>>);

impl ParamsVerifier {
    /// Reads in verifier parameters
    pub fn read<R: Read>(reader: R) -> io::Result<Self> {
        Ok(ParamsProver::read(reader)?.as_verifier())
    }
}

const PARAMS_VERIFIER_RAW: &[u8] = include_bytes!("../../static/bls_filecoin_2p14");

lazy_static! {
    /// The filecoin verifier parameters, up to [`VERIFIER_MAX_DEGREE`].
    ///
    /// Note that using this *will* embed these into the binary at compile time, if that's not what
    /// you want, please use `ParamsVerifier::read` instead.
    pub static ref PARAMS_VERIFIER: ParamsVerifier = ParamsVerifier::read(PARAMS_VERIFIER_RAW).expect("Static verifier parameters should be valid.");
}

/// A zero-knowledge proof.
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serializable, Storable)]
#[storable(base)]
#[tag = "proof[v4]"]
pub struct Proof(pub Vec<u8>);
tag_enforcement_test!(Proof);

/// A prover key, used for creating proofs.
#[derive(Debug, Clone)]
pub struct ProverKey<T: Zkir>(Arc<Mutex<InnerProverKey<T>>>);

impl<T: Zkir> From<MidnightPK<T>> for ProverKey<T> {
    fn from(pk: MidnightPK<T>) -> Self {
        ProverKey(Arc::new(Mutex::new(InnerProverKey::Initialized(Arc::new(
            pk,
        )))))
    }
}

/// An intermediate representation for Midnight's circuits.
#[allow(async_fn_in_trait)]
pub trait Zkir: Relation + Tagged + Deserializable + Any + Send + Sync + Debug {
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
    fn k(&self) -> u8 {
        MidnightCircuit::from_relation(self).min_k() as u8
    }

    /// Performs key generation on this circuit, outputting the verifier key
    async fn keygen_vk(
        &self,
        params: &impl ParamsProverProvider,
    ) -> Result<VerifierKey, anyhow::Error> {
        use midnight_circuits::compact_std_lib::setup_vk;
        let vk = VerifierKey::from(setup_vk(params.get_params(self.k()).await?.as_ref(), self));

        Ok(vk)
    }

    /// Performs key generation on this circuit, outputting the prover/verifier
    /// key pair
    async fn keygen(
        &self,
        params: &impl ParamsProverProvider,
    ) -> Result<(ProverKey<Self>, VerifierKey), anyhow::Error> {
        use midnight_circuits::compact_std_lib::{setup_pk, setup_vk};
        let vk = setup_vk(params.get_params(self.k()).await?.as_ref(), self);
        let pk = setup_pk(self, &vk);

        Ok((ProverKey::from(pk), VerifierKey::from(vk)))
    }
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
    Initialized(Arc<MidnightPK<T>>),
}

impl<T: Zkir> Tagged for ProverKey<T> {
    fn tag() -> Cow<'static, str> {
        Cow::Owned(format!("prover-key[v5]({})", T::tag()))
    }
    fn tag_unique_factor() -> String {
        format!("prover-key[v5]({})", T::tag())
    }
}

const PK_COMPRESSION_LEVEL: u32 = 6;
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
    /// Initializes the lazy prover key
    pub fn init(&self) -> Result<Arc<MidnightPK<T>>, ProvingError> {
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
        let inner_reader = &mut &data[..];
        let mut reader = flate2::read::GzDecoder::new(inner_reader);
        let read_inner = |reader| {
            let pk = MidnightPK::<T>::read(reader, SerdeFormat::RawBytesUnchecked)?;
            Ok(pk)
        };
        let res: Result<_, ProvingError> = read_inner(&mut reader);
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
            InnerProverKey::Initialized(key) => {
                let mut writer = flate2::write::GzEncoder::new(
                    writer,
                    flate2::Compression::new(PK_COMPRESSION_LEVEL),
                );
                key.write(&mut writer, SerdeFormat::RawBytesUnchecked)
            }
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

/// A verifier key, used for checking proofs.
#[derive(Debug, Storable)]
#[storable(base)]
pub struct VerifierKey(Arc<Mutex<InnerVerifierKey>>);

#[cfg(feature = "proptest")]
simple_arbitrary!(VerifierKey);

impl Tagged for VerifierKey {
    fn tag() -> Cow<'static, str> {
        Cow::Borrowed("verifier-key[v4]")
    }
    fn tag_unique_factor() -> String {
        "verifier-key[v4]".into()
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
        VerifierKey(Arc::new(Mutex::new(InnerVerifierKey::Initialized(vk))))
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Some features don't try to initialize
#[allow(clippy::large_enum_variant)]
pub(crate) enum InnerVerifierKey {
    Uninitialized(Vec<u8>),
    Invalid(Vec<u8>),
    Initialized(MidnightVK),
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
    type Instance = Vec<outer::Scalar>;
    type Witness = ();
    fn format_instance(instance: &Self::Instance) -> Vec<outer::Scalar> {
        instance.clone()
    }
    fn circuit(
        &self,
        _std_lib: &midnight_circuits::compact_std_lib::ZkStdLib,
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
            InnerVerifierKey::Initialized(key) => {
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
        *mutex = InnerVerifierKey::Initialized(vk.clone());
        Ok(vk)
    }

    fn inner_serialize<W: std::io::Write>(&self, mut writer: W) -> std::io::Result<()> {
        match &*self.0.lock().expect("mutex is not poisoned") {
            InnerVerifierKey::Uninitialized(data) | InnerVerifierKey::Invalid(data) => {
                writer.write_all(data)
            }
            InnerVerifierKey::Initialized(key) => key.write(&mut writer, SerdeFormat::Processed),
        }
    }

    /// Checks a proof against a statement.
    pub fn verify<F: Iterator<Item = Fr>>(
        &self,
        params: &ParamsVerifier,
        proof: &Proof,
        statement: F,
    ) -> Result<(), VerifyingError> {
        use midnight_circuits::compact_std_lib;

        let vk = self.force_init()?;
        let pi = statement.map(|f| f.0).collect::<Vec<_>>();
        trace!(statement = ?pi, "verifying proof against statement");
        compact_std_lib::verify::<DummyRelation, TranscriptHash>(&params.0, &vk, &pi, &proof.0)
            .map_err(|_| anyhow::anyhow!("Invalid proof"))
    }

    /// Mocks the checking of a proof against a statement
    ///
    /// We do this by running a number of CPU burn cycles calculated to be approximately
    /// equivalent in time-taken to real proof verification
    #[cfg(feature = "mock-verify")]
    pub fn mock_verify<F: Iterator<Item = Fr>>(&self, statement: F) -> Result<(), VerifyingError> {
        let pi_len = statement.count();
        crate::mock_verify::mock_verify_for(pi_len)
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
        use midnight_circuits::compact_std_lib::batch_verify;

        let mut params_verifier = vec![];
        let mut vks = vec![];
        let mut pis = vec![];
        let mut proofs = vec![];

        for (vk, proof, stmt) in parts.into_iter() {
            let pi = stmt.map(|f| f.0).collect::<Vec<_>>();
            let vk = vk.force_init()?;
            params_verifier.push((*params.0).clone());
            vks.push(vk);
            pis.push(pi);
            proofs.push(proof.0.clone());
        }

        batch_verify::<TranscriptHash>(&params_verifier, &vks, &pis, &proofs)
            .map_err(|_| anyhow::anyhow!("Invalid proof"))
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
        for (vk, _proof, stmt) in parts {
            vk.mock_verify(stmt)?;
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
}

/// Everything necessary to produce a proof.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serializable, Hash, Storable)]
#[storable(base)]
#[tag = "proof-preimage"]
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
                &self.key_location.0
            )))?;
        let ir = tagged_deserialize::<Z>(&mut &proof_data.ir_source[..])?;
        let verifier_key = tagged_deserialize::<VerifierKey>(&mut &proof_data.verifier_key[..])?;
        let prover_key = tagged_deserialize::<ProverKey<Z>>(&mut &proof_data.prover_key[..])?;
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
