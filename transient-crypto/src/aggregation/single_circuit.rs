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

//! IVC aggregation of proofs that all share one inner verifying key.
//!
//! Mirrors `midnight-zk`'s `single_circuit_aggregation` example, generalised
//! over any [`AggregableRelation`]. Unlike the multi-circuit aggregator, the
//! inner VK lives in the IVC *context* rather than in each step's witness:
//!
//! - in-circuit, the VK is a hard-coded constant (`assign_fixed_vk`), so no
//!   VK commitments are witnessed or hashed per step;
//! - the IVC verifying key itself pins the inner VK, so the verifier does
//!   not need to check per-claim VKs against a trusted set;
//! - the decider recomputes the hash chain with one 2-input Poseidon per
//!   statement (plus [`AggregableRelation::format_statement`]) instead of
//!   additionally hashing the whole inner VK per claim.
//!
//! The trade-off is one IVC setup (and one IVC verifying key) per inner
//! circuit. This suits homogeneous batches such as Dust spends.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::marker::PhantomData;

use ff::Field;
use group::Group;
use midnight_aggregation::{
    ivc::{self, IvcContext, IvcIO, IvcInstance, IvcState, IvcTransition},
    multi_circuit_aggregator::AggregableRelation,
};
use midnight_circuits::{
    hash::poseidon::{PoseidonChip, PoseidonState},
    instructions::{AssignmentInstructions, PublicInputInstructions, hash::HashCPU},
    types::{AssignedNative, Instantiable},
    verifier::{
        self, Accumulator, AssignedAccumulator, AssignedKZGCommitment, AssignedVk, BlstrsEmulation,
        InCircuitKZG, SelfEmulation,
    },
};
use midnight_proofs::{
    circuit::{Layouter, Value},
    plonk::{self, Error},
    poly::{
        PolynomialLabel,
        kzg::{
            KZGCommitmentScheme, commitment::KZGCommitment, params::ParamsKZG,
            params::ParamsVerifierKZG,
        },
    },
    transcript::{CircuitTranscript, Transcript},
    utils::SerdeFormat,
};
use midnight_zk_stdlib::{MidnightVK, ZkStdLib, ZkStdLibArch};

type S = BlstrsEmulation;
type F = <S as SelfEmulation>::F;
type C = <S as SelfEmulation>::C;
type E = <S as SelfEmulation>::Engine;

/// Setup data for the single inner circuit, threaded as IVC context.
#[derive(Clone, Debug)]
pub struct SingleCircuitContext {
    vk: MidnightVK,
    params_verifier: ParamsVerifierKZG<E>,
    fixed_bases: BTreeMap<PolynomialLabel, C>,
}

impl SingleCircuitContext {
    /// Creates the context for aggregating proofs under `vk`, using the
    /// inner-circuit SRS verifier parameters `params_verifier`.
    pub fn new(vk: MidnightVK, params_verifier: ParamsVerifierKZG<E>) -> Self {
        let fixed_bases = verifier::fixed_bases::<S>(vk.vk());
        SingleCircuitContext {
            vk,
            params_verifier,
            fixed_bases,
        }
    }

    /// The inner verifying key every aggregated proof is checked against.
    pub fn vk(&self) -> &MidnightVK {
        &self.vk
    }

    fn fixed_base_labels(&self) -> Vec<PolynomialLabel> {
        self.fixed_bases.keys().cloned().collect()
    }
}

/// Off-circuit IVC state: every aggregated statement plus constant-size
/// summaries (a Poseidon hash chain and the inner-proof accumulator).
#[derive(Clone, Debug)]
pub struct SingleCircuitState<R: AggregableRelation> {
    statements: Vec<R::Instance>,
    statements_hash: F,
    inner_acc: Accumulator<S>,
}

impl<R: AggregableRelation> SingleCircuitState<R> {
    /// The aggregated statements, in folding order.
    pub fn statements(&self) -> &[R::Instance] {
        &self.statements
    }
}

/// In-circuit counterpart of [`SingleCircuitState`] (constant size).
#[derive(Clone, Debug)]
pub struct AssignedSingleCircuitState {
    statements_hash: AssignedNative<F>,
    inner_acc: AssignedAccumulator<S>,
}

/// Witness for one aggregation step: an inner statement and its proof.
#[derive(Clone, Debug)]
pub struct SingleCircuitWitness<R: AggregableRelation> {
    statement: R::Instance,
    inner_proof: Vec<u8>,
}

impl<R: AggregableRelation> SingleCircuitWitness<R> {
    /// Pairs an inner statement with the proof of it under the context VK.
    pub fn new(statement: R::Instance, inner_proof: Vec<u8>) -> Self {
        SingleCircuitWitness {
            statement,
            inner_proof,
        }
    }
}

/// IVC transition folding one proof of the inner relation `R` per step.
#[derive(Clone, Debug)]
pub struct SingleCircuitAggregation<R> {
    std_lib: ZkStdLib,
    inner_ctx: SingleCircuitContext,
    _relation: PhantomData<fn() -> R>,
}

/// Stateful aggregator for [`SingleCircuitAggregation`].
pub type SingleCircuitAggregator<R> = ivc::IvcProver<SingleCircuitAggregation<R>>;

/// Verifier for [`SingleCircuitAggregation`] proofs.
pub type SingleCircuitVerifier<R> = ivc::IvcVerifier<SingleCircuitAggregation<R>>;

/// Instance (claimed final state) of a [`SingleCircuitAggregation`] proof.
pub type SingleCircuitInstance<R> = IvcInstance<SingleCircuitAggregation<R>>;

impl<R> SingleCircuitAggregation<R>
where
    R: AggregableRelation + Debug,
    R::Instance: Debug,
{
    /// Sets up the aggregator, returning an [`SingleCircuitAggregator`] (at
    /// genesis) and a [`SingleCircuitVerifier`].
    pub fn setup(
        aggregator_srs: ParamsKZG<E>,
        aggregator_k: u32,
        inner_ctx: SingleCircuitContext,
    ) -> (SingleCircuitAggregator<R>, SingleCircuitVerifier<R>) {
        ivc::setup::<Self>(aggregator_srs, aggregator_k, inner_ctx)
    }

    fn hash_step(statement: F, prev: F) -> F {
        <PoseidonChip<F> as HashCPU<F, F>>::hash(&[statement, prev])
    }
}

impl<R> IvcContext for SingleCircuitAggregation<R>
where
    R: AggregableRelation + Debug,
    R::Instance: Debug,
{
    type Context = SingleCircuitContext;

    fn new(std_lib: ZkStdLib, ctx: &SingleCircuitContext) -> Self {
        SingleCircuitAggregation {
            std_lib,
            inner_ctx: ctx.clone(),
            _relation: PhantomData,
        }
    }

    fn write_context<W: std::io::Write>(
        ctx: &SingleCircuitContext,
        writer: &mut W,
    ) -> std::io::Result<()> {
        ctx.vk.write(writer, SerdeFormat::RawBytes)?;
        ctx.params_verifier.write(writer, SerdeFormat::RawBytes)
    }

    fn read_context<Rd: std::io::Read>(reader: &mut Rd) -> std::io::Result<SingleCircuitContext> {
        let vk = MidnightVK::read(reader, SerdeFormat::RawBytes)?;
        let params_verifier = ParamsVerifierKZG::read(reader, SerdeFormat::RawBytes)?;
        Ok(SingleCircuitContext::new(vk, params_verifier))
    }
}

impl<R> IvcState for SingleCircuitAggregation<R>
where
    R: AggregableRelation + Debug,
    R::Instance: Debug,
{
    type State = SingleCircuitState<R>;
    type AssignedState = AssignedSingleCircuitState;

    fn genesis(ctx: &SingleCircuitContext) -> Self::State {
        SingleCircuitState {
            statements: vec![],
            statements_hash: F::ZERO,
            inner_acc: Accumulator::<S>::trivial(&ctx.fixed_base_labels()),
        }
    }

    fn decider(ctx: &SingleCircuitContext, state: &Self::State) -> bool {
        let statements_hash = state.statements.iter().fold(F::ZERO, |h_acc, x| {
            Self::hash_step(R::format_statement(x), h_acc)
        });
        if statements_hash != state.statements_hash {
            return false;
        }
        state
            .inner_acc
            .check(&ctx.params_verifier, &ctx.fixed_bases)
    }
}

impl<R> IvcIO for SingleCircuitAggregation<R>
where
    R: AggregableRelation + Debug,
    R::Instance: Debug,
{
    fn assign(
        &self,
        layouter: &mut impl Layouter<F>,
        value: Value<Self::State>,
    ) -> Result<Self::AssignedState, Error> {
        let statements_hash = self
            .std_lib
            .assign(layouter, value.as_ref().map(|s| s.statements_hash))?;
        let inner_acc = self.std_lib.verifier().assign_collapsed_accumulator(
            layouter,
            &self.inner_ctx.fixed_base_labels(),
            value.as_ref().map(|s| s.inner_acc.clone()),
        )?;
        Ok(AssignedSingleCircuitState {
            statements_hash,
            inner_acc,
        })
    }

    fn constrain_as_public_input(
        &self,
        layouter: &mut impl Layouter<F>,
        state: &Self::AssignedState,
    ) -> Result<(), Error> {
        self.std_lib
            .constrain_as_public_input(layouter, &state.statements_hash)?;
        self.std_lib
            .verifier()
            .constrain_as_public_input(layouter, &state.inner_acc)
    }

    fn as_public_input(
        &self,
        layouter: &mut impl Layouter<F>,
        state: &Self::AssignedState,
    ) -> Result<Vec<AssignedNative<F>>, Error> {
        Ok([
            self.std_lib
                .as_public_input(layouter, &state.statements_hash)?,
            self.std_lib
                .verifier()
                .as_public_input(layouter, &state.inner_acc)?,
        ]
        .concat())
    }

    fn format_public_input(state: &Self::State) -> Vec<F> {
        [
            vec![state.statements_hash],
            AssignedAccumulator::<S>::as_public_input(&state.inner_acc),
        ]
        .concat()
    }
}

impl<R> IvcTransition for SingleCircuitAggregation<R>
where
    R: AggregableRelation + Debug,
    R::Instance: Debug,
{
    type Witness = SingleCircuitWitness<R>;

    fn arch() -> ZkStdLibArch {
        ZkStdLibArch {
            poseidon: true,
            nr_pow2range_cols: 4,
            ..ZkStdLibArch::default()
        }
    }

    fn transition(
        ctx: &SingleCircuitContext,
        state: &Self::State,
        witness: Self::Witness,
    ) -> Self::State {
        let statement = R::format_statement(&witness.statement);

        let inner_proof_acc = {
            let mut transcript =
                CircuitTranscript::<PoseidonState<F>>::init_from_bytes(&witness.inner_proof);
            let dual_msm =
                plonk::prepare::<F, KZGCommitmentScheme<E>, CircuitTranscript<PoseidonState<F>>>(
                    ctx.vk.vk(),
                    &[KZGCommitment::Simple(
                        C::identity(),
                        PolynomialLabel::Instance(0),
                    )],
                    &[&[statement]],
                    &mut transcript,
                )
                .expect("off-circuit prepare should succeed");

            assert!(
                dual_msm.clone().check(&ctx.params_verifier),
                "invalid inner proof"
            );

            Accumulator::from_dual_msm(dual_msm, &ctx.fixed_bases)
        };

        let inner_acc = {
            let mut acc = Accumulator::accumulate(&[inner_proof_acc, state.inner_acc.clone()]);
            acc.collapse();
            acc
        };

        let statements_hash = Self::hash_step(statement, state.statements_hash);

        let mut statements = state.statements.clone();
        statements.push(witness.statement);

        SingleCircuitState {
            statements,
            statements_hash,
            inner_acc,
        }
    }

    fn circuit_transition(
        &self,
        layouter: &mut impl Layouter<F>,
        state: &Self::AssignedState,
        witness: Value<Self::Witness>,
    ) -> Result<Self::AssignedState, Error> {
        // The inner VK is a circuit constant, pinned by the IVC verifying key.
        // Its (finalised, selector-free) constraint system and domain come
        // straight from the VK, as `assign_fixed_vk` expects.
        let vk = self.inner_ctx.vk.vk();
        let inner_vk: AssignedVk<S, InCircuitKZG<S>> = self.std_lib.verifier().assign_fixed_vk(
            layouter,
            vk.get_domain(),
            vk.cs(),
            vk.transcript_repr(),
        )?;

        let statement: AssignedNative<F> = self.std_lib.assign(
            layouter,
            witness.as_ref().map(|w| R::format_statement(&w.statement)),
        )?;

        let instance_com = AssignedKZGCommitment::<S>::simple(
            self.std_lib
                .bls12_381()
                .assign_fixed(layouter, C::identity())?,
            PolynomialLabel::CommittedInstance(0),
        );
        let inner_proof_acc = self.std_lib.verifier().prepare(
            layouter,
            &inner_vk,
            &[instance_com],
            &[std::slice::from_ref(&statement)],
            witness.map(|w| w.inner_proof),
        )?;

        let inner_acc = {
            let mut acc = self
                .std_lib
                .verifier()
                .accumulate(layouter, &[inner_proof_acc, state.inner_acc.clone()])?;
            acc.collapse(
                layouter,
                self.std_lib.bls12_381(),
                self.std_lib.bls12_381().scalar_field_chip(),
            )?;
            acc
        };

        let statements_hash = self
            .std_lib
            .poseidon(layouter, &[statement, state.statements_hash.clone()])?;

        Ok(AssignedSingleCircuitState {
            statements_hash,
            inner_acc,
        })
    }
}
