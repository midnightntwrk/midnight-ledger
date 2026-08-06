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

//! An IVC-aggregation-compatible wrapper around [`IrSource`].
//!
//! [`midnight_aggregation`]'s multi-circuit aggregator only accepts inner
//! circuits that implement `AggregableRelation`, which requires (per its
//! docs) that the circuit "format its instance into a single public input".
//! `IrSource` itself exposes an arbitrary number of public inputs (the
//! binding input, optionally the communications commitment, and one per
//! `PublicInput` instruction) — see [`IrSource::assign_public_inputs`] — so
//! it cannot implement `AggregableRelation` directly without changing what
//! every existing V3 proof commits to.
//!
//! [`AggregableIrSource`] resolves this without touching `IrSource`'s own
//! `Relation` impl (and therefore without changing its proof shape or VKs):
//! it reuses [`IrSource::assign_public_inputs`] for the exact same in-circuit
//! logic, then hashes the resulting public-input vector down to a single
//! value with the same Poseidon instance used elsewhere in this codebase for
//! commitments (`std.poseidon` in-circuit, [`transient_hash`] off-circuit —
//! see [`aggregation_digest`]).
//!
//! # Known limitation: one shared architecture for every aggregated circuit
//!
//! [`midnight_aggregation::multi_circuit_aggregator::AggregationWitness::new`]
//! checks the inner circuit's architecture via `R::default().used_chips()`
//! — a *type-level*, not instance-level, property. `IrSource::used_chips`
//! is instance-dependent (it inspects `self.instructions`), so
//! `AggregableIrSource::default()` (wrapping a blank, instruction-less
//! `IrSource`) is not representative of any real circuit's actual chip
//! usage. [`AggregableIrSource::used_chips`] therefore returns a **fixed**
//! architecture (see [`AggregableIrSource::aggregation_arch`]) rather than
//! delegating to the wrapped `IrSource`. Every real circuit aggregated
//! through this type must only use chips within that fixed set, and the same
//! value must be passed as the `arch` when building the
//! [`midnight_aggregation::multi_circuit_aggregator::InnerCircuitsContext`].
//! Widen [`AggregableIrSource::aggregation_arch`] if a circuit needs a chip
//! it doesn't currently include. This mirrors how the aggregation crate's
//! own multi-circuit example picks one union architecture upfront for every
//! circuit it will ever aggregate.
//!
//! This module, and everything that depends on it, is new and has not been
//! reviewed by whoever owns the aggregation circuit design — treat it as a
//! prototype, not as something to ship.

use midnight_circuits::instructions::PublicInputInstructions;
use midnight_circuits::types::AssignedNative;
use midnight_proofs::circuit::{Layouter, Value};
use midnight_proofs::plonk::Error;
use midnight_zk_stdlib::{Relation, ZkStdLib, ZkStdLibArch};
use transient_crypto::aggregation::AggregableRelation;
use transient_crypto::curve::{Fr, outer};
use transient_crypto::hash::transient_hash;

use crate::ir::IrSource;
use crate::ir_vm::Preprocessed;

/// Wraps an [`IrSource`] so it can be aggregated via
/// [`midnight_aggregation::multi_circuit_aggregator::Aggregator`].
///
/// See the module docs for why this exists as a separate type instead of an
/// `AggregableRelation` impl on `IrSource` directly, and for the
/// shared-architecture caveat.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct AggregableIrSource(pub IrSource);

impl AggregableIrSource {
    /// The single [`ZkStdLibArch`] every circuit aggregated through this
    /// wrapper must be compiled with — this is what gets passed to
    /// `InnerCircuitsContext::new` and is what
    /// [`AggregableIrSource::used_chips`] unconditionally returns (see the
    /// module docs for why it can't be derived from the wrapped `IrSource`
    /// instead). Extend this (add more `true` fields) if a circuit you want
    /// to aggregate needs a chip that isn't already enabled here; every
    /// circuit sharing this wrapper type must then be recompiled against
    /// the widened architecture.
    pub fn aggregation_arch() -> ZkStdLibArch {
        ZkStdLibArch {
            poseidon: true,
            ..ZkStdLibArch::default()
        }
    }

    /// Public entry point to [`IrSource::preprocess`] (crate-private on
    /// `IrSource` itself), so external callers — e.g. code building
    /// [`AggregationWitness`](transient_crypto::aggregation::AggregationWitness)
    /// values — can obtain a [`Preprocessed`] witness without going through
    /// the `Zkir::prove`/`Zkir::check` convenience wrappers, which are
    /// specific to `IrSource`'s own (non-aggregated) `Relation` impl.
    ///
    /// The returned `pis` field is exactly the `instance` this witness must
    /// be paired with when calling `Relation::format_instance` /
    /// `AggregableRelation::format_statement` — see [`aggregation_digest`].
    pub fn preprocess(
        &self,
        preimage: &transient_crypto::proofs::ProofPreimage,
    ) -> Result<Preprocessed, transient_crypto::proofs::ProvingError> {
        self.0.preprocess(preimage)
    }
}

/// Off-circuit equivalent of hashing `pis` via `std.poseidon` in-circuit.
///
/// Uses [`transient_hash`], which wraps the exact same Poseidon instance as
/// the `std.poseidon` gadget (this is the same correspondence the rest of
/// this codebase relies on for the communications-commitment check in
/// [`IrSource::assign_public_inputs`]). Must be called with the same values,
/// in the same order, as the `public_inputs` vector `assign_public_inputs`
/// assembles in-circuit — by convention (enforced by `IrSource::preprocess`'s
/// consistency check) that's exactly `Preprocessed::pis`, i.e. the `instance`
/// every caller is expected to pass to `Relation::format_instance`.
pub fn aggregation_digest(instance: &[outer::Scalar]) -> outer::Scalar {
    transient_hash(&instance.iter().copied().map(Fr).collect::<Vec<_>>()).0
}

impl Relation for AggregableIrSource {
    type Instance = Vec<outer::Scalar>;
    type Witness = Preprocessed;
    type Error = Error;

    /// Returns the *hashed* instance (a single value), not the raw
    /// multi-value `instance` — this is what actually gets fed to the
    /// underlying prover as the circuit's public input, so it must match
    /// what `circuit` below constrains. `AggregableRelation::format_statement`'s
    /// default implementation asserts there is exactly one, which is
    /// exactly why this is a single-element vector rather than `instance`
    /// unchanged.
    fn format_instance(instance: &Self::Instance) -> Result<Vec<outer::Scalar>, Error> {
        Ok(vec![aggregation_digest(instance)])
    }

    fn circuit(
        &self,
        std: &ZkStdLib,
        layouter: &mut impl Layouter<outer::Scalar>,
        instance: Value<Self::Instance>,
        witness: Value<Self::Witness>,
    ) -> Result<(), Error> {
        let public_inputs: Vec<AssignedNative<outer::Scalar>> =
            self.0.assign_public_inputs(std, layouter, instance, witness)?;
        let digest = std.poseidon(layouter, &public_inputs)?;
        std.constrain_as_public_input(layouter, &digest)
    }

    /// Fixed, not derived from `self.0` — see the module docs and
    /// [`AggregableIrSource::aggregation_arch`]: the aggregator checks this
    /// via `Self::default().used_chips()`, so an instance-dependent answer
    /// would silently be ignored in favor of the blank default's.
    fn used_chips(&self) -> ZkStdLibArch {
        Self::aggregation_arch()
    }

    fn write_relation<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.write_relation(writer)
    }

    fn read_relation<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        IrSource::read_relation(reader).map(AggregableIrSource)
    }
}

/// `format_instance` already returns a single value, so the default
/// `format_statement` (which calls `format_instance` and asserts there's
/// exactly one) is correct as-is.
impl AggregableRelation for AggregableIrSource {}
