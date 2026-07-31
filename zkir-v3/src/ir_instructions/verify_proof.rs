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
use midnight_circuits::types::{AssignedNative, Instantiable};
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
use transient_crypto::proofs::S;

/// A fixed, arbitrary name used to label the inner verifying key's fixed bases.
const VK_NAME: &str = "inner_vk";

/// Number of public-input field elements occupied by one fully-collapsed,
/// single-point-per-side accumulator.
pub fn accumulator_pi_len() -> usize {
    <AssignedAccumulator<S> as Instantiable<outer::Scalar>>::as_public_input(
        &Accumulator::<S>::trivial(&[]),
    )
    .len()
}

/// Off-circuit partial verification of an inner proof into a single-point
/// accumulator, encoded as public-input field elements.
pub fn verify_proof_offcircuit(
    vk_blob: &[u8],
    instance: &[outer::Scalar],
    proof: &[u8],
) -> anyhow::Result<Vec<outer::Scalar>> {
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
pub fn verify_proof_incircuit(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<outer::Scalar>,
    vk_blob: &[u8],
    instance: &[&[AssignedNative<outer::Scalar>]],
    proof: Value<Vec<u8>>,
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
    acc.collapse(layouter, bls, bls.scalar_field_chip())?;

    verifier.constrain_as_public_input(layouter, &acc)?;
    Ok(())
}
