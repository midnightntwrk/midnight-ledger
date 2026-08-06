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
        AggregationWitness, Aggregator, InnerCircuitsContext, ProofAggregation,
        Verifier as AggregationVerifier,
    },
};

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
