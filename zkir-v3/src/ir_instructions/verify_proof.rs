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
//! may have a deciding function listed in [`crate::ir_instructions::decider`]. 
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
use sha2::{Digest, Sha256};
use transient_crypto::curve::outer;
use transient_crypto::proofs::InnerSelfEmulation as S;

use crate::ir_instructions::decider::{decide_incircuit, decide_offcircuit, deserialize_vk};

/// Label prefix for an inner verifying key's fixed bases.
///
/// Derived from the blob's own digest.
fn vk_name(vk_blob: &[u8]) -> String {
    format!("inner_vk_{}", const_hex::encode(Sha256::digest(vk_blob)))
}

/// Off-circuit half of `verify_proof`: partially verifies the inner proof and
/// returns the accumulator the check defers. There is no verdict to return --
/// the final pairing is left to the outer verifier, which reads the accumulator
/// back from the public inputs.
///
/// If `guard` is `false` neither key nor proof is read and the trivial
/// accumulator is returned. That exit is required, not an optimisation: a
/// guarded-off `inner_proof` binds the empty blob, on which `plonk::prepare`
/// fails at EOF.
pub fn verify_proof_offcircuit(
    vk_blob: &[u8],
    instance: &[outer::Scalar],
    proof: &[u8],
    guard: bool,
) -> anyhow::Result<Accumulator<S>> {
    if !guard {
        return Ok(Accumulator::<S>::trivial(&[]));
    }

    let (kind, vk) = deserialize_vk(vk_blob)?;
    let plonk_vk = vk.vk();
    let vk_name = vk_name(vk_blob);
    let bases = fixed_bases::<S>(&vk_name, plonk_vk);

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

    let own_acc = Accumulator::<S>::from_dual_msm(dual_msm, &vk_name, &bases);
    decide_offcircuit(kind, own_acc, &bases, instance)
}

/// In-circuit half of [`verify_proof_offcircuit`]: verifies the inner proof and
/// constrains the accumulator as public inputs. Not the same shape -- it emits
/// constraints where the other returns values -- but it constrains exactly the
/// fields the other returns.
///
/// The proof is prepared whatever `guard` is, since the circuit's shape cannot
/// depend on a witness; if `guard` is `0` the accumulator is reduced to the
/// trivial one, so the deferred pairing holds whatever the prover supplied.
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
    let vk_name = vk_name(vk_blob);
    let verifier = std.verifier();
    let bls = std.bls12_381();

    let assigned_vk = verifier.assign_fixed_vk(
        layouter,
        &vk_name,
        plonk_vk.get_domain(),
        plonk_vk.cs(),
        plonk_vk.transcript_repr(),
    )?;

    // Assign the inner VK's fixed bases in-circuit, keyed by the same names
    // `fixed_bases` produces, so `resolve_fixed_bases` can match them.
    let mut assigned_bases = BTreeMap::new();
    for (name, base) in fixed_bases::<S>(&vk_name, plonk_vk) {
        assigned_bases.insert(name, bls.assign_fixed(layouter, base)?);
    }

    // The committed instance is a single identity point (we do not support
    // committed instances), mirroring the off-circuit `&[&[C::identity()]]`.
    let committed = [bls.assign_fixed(layouter, <S as SelfEmulation>::C::identity())?];

    let own_acc = verifier.prepare(layouter, &assigned_vk, &committed, instance, proof)?;

    // Exactly one instance set, matching the off-circuit `&[&[instance]]`.
    let [fields] = instance else {
        return Err(Error::Synthesis(
            "`verify_proof` supports exactly one instance set".into(),
        ));
    };
    decide_incircuit(std, layouter, kind, own_acc, &assigned_bases, fields, guard)
}
