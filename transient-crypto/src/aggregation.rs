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
#![cfg(feature = "proof-aggregation")]

//! IVC proof aggregation support.
//!
//! Re-exports [`midnight_aggregation`] types and defines the thin wrapper types
//! used to carry pre-aggregated proofs in transactions.
//!
//! The upper layer is responsible for:
//! - constructing the [`AggregationVerifier`] with the runtime SRS via
//!   [`ProofAggregation::setup`],
//! - implementing [`AggregationVerify`] to bridge the ledger's byte-level
//!   interface to the concrete [`IvcInstance`].

pub use midnight_aggregation::{
    ivc::{IvcError, IvcInstance, IvcVerifier, setup as ivc_setup},
    multi_circuit_aggregator::{
        AggregableRelation, AggregationWitness, Aggregator, InnerCircuitsContext, ProofAggregation,
        Verifier as AggregationVerifier,
    },
};
pub use midnight_circuits::hash::poseidon::PoseidonState as AggregationTranscript;
pub use midnight_zk_stdlib::MidnightVK;

use midnight_curves::Bls12;
use midnight_proofs::{
    poly::kzg::params::{ParamsKZG, ParamsVerifierKZG},
    utils::SerdeFormat,
};

/// Returns the K=14 inner-circuit verifier params from the embedded Midnight SRS.
///
/// Suitable for passing to [`InnerCircuitsContext::new`] when aggregating V3 proofs
/// produced by zkir-v3 circuits compiled against the default Midnight trusted setup.
pub fn inner_verifier_params() -> ParamsVerifierKZG<Bls12> {
    crate::proofs::ParamsProver::read(crate::proofs::PARAMS_VERIFIER_RAW)
        .expect("embedded inner SRS bytes are valid")
        .0
        .verifier_params()
}

/// Returns the full K=14 prover params from the embedded Midnight SRS.
///
/// The prover params are a superset of the verifier params returned by
/// [`inner_verifier_params`]. Pass these to `midnight_zk_stdlib::setup_vk`,
/// `midnight_zk_stdlib::setup_pk`, and `midnight_zk_stdlib::prove` when proving
/// inner circuits (e.g. [`zkir_v3::AggregableIrSource`]) for aggregation without
/// loading an external SRS file.
pub fn inner_prover_params() -> std::sync::Arc<ParamsKZG<Bls12>> {
    crate::proofs::ParamsProver::read(crate::proofs::PARAMS_VERIFIER_RAW)
        .expect("embedded inner SRS bytes are valid")
        .0
}

/// Reads `{dir}/bls_midnight_2p{k}` and returns the full [`ParamsKZG`].
///
/// Intended for loading the outer aggregator SRS (typically K=19) before
/// calling [`ProofAggregation::setup`]. Returns an error if the file cannot
/// be opened or parsed.
pub fn load_midnight_srs(dir: &std::path::Path, k: u32) -> std::io::Result<ParamsKZG<Bls12>> {
    let path = dir.join(format!("bls_midnight_2p{k}"));
    let mut reader = std::io::BufReader::new(std::fs::File::open(&path)?);
    Ok(ParamsKZG::<Bls12>::read_custom(
        &mut reader,
        SerdeFormat::RawBytesUnchecked,
    )?)
}

/// Opaque pre-aggregated IVC proof bundle for inclusion in a transaction.
///
/// Produced by the prover using [`Aggregator::aggregate`]. The upper layer
/// verifies it by implementing [`AggregationVerify`] on top of a runtime
/// [`AggregationVerifier`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AggregatedContractProof {
    /// Raw IVC proof bytes (output of [`Aggregator::aggregate`]).
    pub ivc_proof: Vec<u8>,
    /// Serialized [`IvcInstance<ProofAggregation>`] bytes.
    ///
    /// Opaque to the ledger. The upper layer deserializes this back into
    /// [`IvcInstance<ProofAggregation>`] using the appropriate context and
    /// passes it together with [`ivc_proof`](Self::ivc_proof) to the
    /// [`AggregationVerifier`].
    pub ivc_instance: Vec<u8>,
}

/// Verification interface for aggregated IVC proofs.
///
/// Implement this trait on a type that holds a runtime-constructed
/// [`AggregationVerifier`] (and its SRS) and pass it to
/// [`Transaction::verify_aggregated_proofs`].
pub trait AggregationVerify: Send + Sync {
    /// Verifies a single aggregated proof bundle.
    fn verify_aggregated_proof(&self, proof: &AggregatedContractProof)
    -> Result<(), anyhow::Error>;
}
