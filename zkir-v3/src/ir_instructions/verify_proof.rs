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

//! The `verify_proof` instruction: verify an inner Midnight proof, exposing the
//! resulting (deferred) accumulator as public inputs. Built directly on the
//! verifier-gadget primitives that midnight-circuits exposes. The inner proofs
//! may have a deciding function listed in [`crate::ir_instructions::decidable`]. 
//! Other deciding strategies are not supported. The verifier side (reconstructing
//! each accumulator from the public inputs and running its pairing check) lives
//! in `transient-crypto`.

use std::collections::BTreeMap;

use group::Group;
use midnight_circuits::hash::poseidon::PoseidonState;
use midnight_circuits::instructions::AssignmentInstructions;
use midnight_circuits::types::{AssignedBit, AssignedNative};
use midnight_circuits::verifier::{Accumulator, SelfEmulation, fixed_bases};
use midnight_curves::Bls12;
use midnight_proofs::{
    circuit::{Layouter, Value},
    plonk::{self, Error},
    poly::kzg::KZGCommitmentScheme,
    transcript::{CircuitTranscript, Transcript},
};
use midnight_zk_stdlib::ZkStdLib;
use transient_crypto::curve::outer;
use transient_crypto::proofs::InnerSelfEmulation as S;

use crate::ir_instructions::decidable::{
    decide_incircuit, decide_offcircuit, deserialize_vk, trivial_accumulator_pis,
};

/// Label prefix for the inner verifying key's fixed bases. The specific string
/// is arbitrary but must match between the off- and in-circuit passes so
/// `resolve_fixed_bases` can pair each named scalar with its base.
const VK_NAME: &str = "inner_vk";

/// Off-circuit partial verification of an inner proof into a single-point
/// accumulator, encoded as public-input field elements.
///
/// If `guard` is `false` the key is not even read, and the trivial
/// accumulator's encoding is returned instead. That is what the in-circuit pass
/// reduces a guarded-off accumulator to, so the public inputs of the two runs
/// agree.
pub fn verify_proof_offcircuit(
    vk_blob: &[u8],
    instance: &[outer::Scalar],
    proof: &[u8],
    guard: bool,
) -> anyhow::Result<Vec<outer::Scalar>> {
    if !guard {
        return Ok(trivial_accumulator_pis());
    }

    let (kind, vk) = deserialize_vk(vk_blob)?;
    let plonk_vk = vk.vk();
    let bases = fixed_bases::<S>(VK_NAME, plonk_vk);

    let mut transcript = CircuitTranscript::<PoseidonState<outer::Scalar>>::init_from_bytes(proof);
    let dual_msm = plonk::prepare::<
        outer::Scalar,
        KZGCommitmentScheme<Bls12>,
        CircuitTranscript<PoseidonState<outer::Scalar>>,
    >(
        plonk_vk,
        &[&[<S as SelfEmulation>::C::identity()]],
        &[&[instance]],
        &mut transcript,
    )?;

    let own = Accumulator::<S>::from_dual_msm(dual_msm, VK_NAME, &bases);
    decide_offcircuit(kind, own, &bases, instance)
}

/// In-circuit mirror of [`verify_proof_offcircuit`]: verifies the inner proof
/// in-circuit and constrains the resulting single-point accumulator as public
/// inputs.
///
/// The proof is prepared in-circuit whatever `guard` is, but if `guard` is `0` 
/// the accumulator is reduced to the trivial one, so the pairing check the 
/// outer verifier defers holds regardless of what the prover supplied as the
/// proof.
pub fn verify_proof_incircuit(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<outer::Scalar>,
    vk_blob: &[u8],
    instance: &[&[AssignedNative<outer::Scalar>]],
    proof: Value<Vec<u8>>,
    guard: &AssignedBit<outer::Scalar>,
) -> Result<(), Error> {
    let (kind, vk) =
        deserialize_vk(vk_blob).map_err(|e| Error::Synthesis(format!("inner verifying key: {e}")))?;
    let plonk_vk = vk.vk();
    let verifier = std.verifier();
    let bls = std.bls12_381();

    let assigned_vk = verifier.assign_fixed_vk(
        layouter,
        VK_NAME,
        plonk_vk.get_domain(),
        plonk_vk.cs(),
        plonk_vk.transcript_repr(),
    )?;

    // Assign the inner VK's fixed bases in-circuit, keyed by the same names
    // `fixed_bases` produces, so `resolve_fixed_bases` can match them.
    let mut assigned_bases = BTreeMap::new();
    for (name, base) in fixed_bases::<S>(VK_NAME, plonk_vk) {
        assigned_bases.insert(name, bls.assign_fixed(layouter, base)?);
    }

    // The committed instance is a single identity point (we do not support
    // committed instances), mirroring the off-circuit `&[&[C::identity()]]`.
    let committed = [bls.assign_fixed(layouter, <S as SelfEmulation>::C::identity())?];

    let own = verifier.prepare(layouter, &assigned_vk, &committed, instance, proof)?;

    // Exactly one instance set, matching the off-circuit `&[&[instance]]`.
    let [fields] = instance else {
        return Err(Error::Synthesis(
            "`verify_proof` supports exactly one instance set".into(),
        ));
    };
    decide_incircuit(std, layouter, kind, own, &assigned_bases, fields, guard)
}
