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
//! verifier-gadget primitives that midnight-circuits exposes. The verifier
//! side (reconstructing each accumulator from the public inputs and running its
//! pairing check) lives in `transient-crypto`.

use std::collections::BTreeMap;

use anyhow::anyhow;
use group::Group;
use midnight_circuits::hash::poseidon::PoseidonState;
use midnight_circuits::instructions::{AssignmentInstructions, PublicInputInstructions};
use midnight_circuits::types::{AssignedBit, AssignedNative, Instantiable};
use midnight_circuits::verifier::{Accumulator, AssignedAccumulator, SelfEmulation, fixed_bases};
use midnight_curves::Bls12;
use midnight_proofs::{
    circuit::{Layouter, Value},
    plonk::{self, Error},
    poly::kzg::KZGCommitmentScheme,
    transcript::{CircuitTranscript, Transcript},
    utils::SerdeFormat,
};
use midnight_zk_stdlib::{MidnightVK, ZkStdLib};
use transient_crypto::curve::outer;
use transient_crypto::proofs::InnerSelfEmulation as S;

/// Label prefix for the inner verifying key's fixed bases. The specific string
/// is arbitrary but must match between the off- and in-circuit passes so
/// `resolve_fixed_bases` can pair each named scalar with its base.
const VK_NAME: &str = "inner_vk";

/// Public-input encoding of the accumulator a guarded-off `verify_proof`
/// exposes: the trivial accumulator, whose sides both evaluate to the identity
/// and which therefore satisfies the pairing invariant by construction.
///
/// It is a fixed, proof-independent encoding, which is what lets a guarded-off
/// instruction accept any inner proof witness at all — including an empty one.
pub fn trivial_accumulator_pis() -> Vec<outer::Scalar> {
    <AssignedAccumulator<S> as Instantiable<outer::Scalar>>::as_public_input(
        &Accumulator::<S>::trivial(&[]),
    )
}

/// Off-circuit partial verification of an inner proof into a single-point
/// accumulator, encoded as public-input field elements.
///
/// If `guard` is `false` the proof is not read at all, and the trivial
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

    let vk = MidnightVK::read(&mut { vk_blob }, SerdeFormat::Processed)
        .map_err(|e| anyhow!("reading inner verifying key: {e}"))?;
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

    let mut acc = Accumulator::<S>::from_dual_msm(dual_msm, VK_NAME, &bases);
    acc.collapse();
    acc.resolve_fixed_bases(&bases);
    acc.collapse();

    Ok(<AssignedAccumulator<S> as Instantiable<outer::Scalar>>::as_public_input(&acc))
}

/// In-circuit mirror of [`verify_proof_offcircuit`]: verifies the inner proof
/// in-circuit and constrains the resulting single-point accumulator as public
/// inputs.
///
/// The proof is prepared in-circuit whatever `guard` is — that is fixed circuit
/// structure — but if `guard` is `0` the accumulator that preparation produces
/// is reduced to the trivial one, so the pairing check the outer verifier defers
/// on it holds regardless of what (if anything) the prover supplied as the
/// proof. The transcript gadget assigns default points and zero scalars for
/// bytes it cannot read, so an empty witness is admissible.
pub fn verify_proof_incircuit(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<outer::Scalar>,
    vk_blob: &[u8],
    instance: &[&[AssignedNative<outer::Scalar>]],
    proof: Value<Vec<u8>>,
    guard: &AssignedBit<outer::Scalar>,
) -> Result<(), Error> {
    let vk = MidnightVK::read(&mut { vk_blob }, SerdeFormat::Processed)
        .map_err(|e| Error::Synthesis(format!("reading inner verifying key: {e}")))?;
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

    let mut acc = verifier.prepare(layouter, &assigned_vk, &committed, instance, proof)?;
    acc.collapse(layouter, bls, bls.scalar_field_chip())?;
    acc.resolve_fixed_bases(&assigned_bases);
    // Second collapse: `resolve_fixed_bases` moves the inner VK's fixed-base
    // contributions into the variable-base MSM, so collapse again to fold them
    // back into a single point per side.
    acc.collapse(layouter, bls, bls.scalar_field_chip())?;

    // Guard the accumulator. Scaling by the bit zeroes each side's scalar but
    // leaves its (proof-derived) base in place, so collapse once more to
    // evaluate `0 * base` down to the identity. A guarded-on accumulator is
    // unchanged by the pair of operations — each side is a single base with
    // scalar one either way — while a guarded-off one becomes exactly
    // `Accumulator::trivial`: the identity with scalar one, on both sides.
    //
    // A guarded-off instruction therefore exposes a fixed encoding, independent
    // of the proof, that satisfies the pairing invariant by construction. That
    // is what lets the off-circuit pass skip preparation entirely, and so accept
    // any inner proof witness, including an empty one.
    //
    // The scaled scalar is bounded by one, so this second collapse costs a
    // conditional selection and a point addition, not a scalar multiplication.
    AssignedAccumulator::scale_by_bit(layouter, bls.scalar_field_chip(), guard, &mut acc)?;
    acc.collapse(layouter, bls, bls.scalar_field_chip())?;

    verifier.constrain_as_public_input(layouter, &acc)?;
    Ok(())
}
