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

//! Extra verification obligations carried by inner proofs.
//!
//! Some proofs need more than a PLONK verification to be accepted. For example
//! IVC chain proofs require some extra checks on their public inputs.
//!
//! [`DeciderKind`] declares, per inner verifying key, which finishing procedure
//! a given inner proof requires.

use std::collections::BTreeMap;
use std::io;

use anyhow::{anyhow, bail};
use group::ff::Field;
use midnight_circuits::instructions::{
    AssertionInstructions, AssignmentInstructions, ControlFlowInstructions, PublicInputInstructions,
};
use midnight_circuits::types::{AssignedBit, AssignedNative, Instantiable};
use midnight_circuits::verifier::{Accumulator, AssignedAccumulator, SelfEmulation};
use midnight_proofs::{
    circuit::{Layouter, Value},
    plonk::Error,
    utils::SerdeFormat,
};
use midnight_zk_stdlib::{MidnightVK, ZkStdLib};
use transient_crypto::curve::outer;
use transient_crypto::proofs::{
    InnerSelfEmulation as S, accumulator_pi_len, reconstruct_accumulator,
};

type AssignedPoint = <S as SelfEmulation>::AssignedPoint;

/// Which deferred obligation, if any, an inner proof carries into the proof that
/// verifies it.
///
/// # Wire format
///
/// The tag is a single byte written by declaration order: `None = 0`,
/// `Collapsed = 1`. It is consensus wire format, so **never reorder or remove a
/// variant**; only append.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeciderKind {
    /// No accumulator of its own.
    None,
    /// Carries exactly one fully-collapsed, fixed-base-resolved accumulator in
    /// the last [`accumulator_pi_len`] fields of its instance.
    Collapsed,
}

impl DeciderKind {
    fn tag(self) -> u8 {
        match self {
            DeciderKind::None => 0,
            DeciderKind::Collapsed => 1,
        }
    }

    fn from_tag(tag: u8) -> anyhow::Result<Self> {
        match tag {
            0 => Ok(DeciderKind::None),
            1 => Ok(DeciderKind::Collapsed),
            other => bail!(
                "unknown decider tag {other} in a `verify_proof_vks` entry: a ledger that does \
                 not know how to finish a proof must refuse it"
            ),
        }
    }
}

/// Encodes an inner verifying key as `[tag] | MidnightVK::write(Processed)`.
pub fn serialize_vk(vk: &MidnightVK, kind: DeciderKind) -> io::Result<Vec<u8>> {
    let mut blob = vec![kind.tag()];
    vk.write(&mut blob, SerdeFormat::Processed)?;
    Ok(blob)
}

/// Inverse of [`serialize_vk`].
pub fn deserialize_vk(blob: &[u8]) -> anyhow::Result<(DeciderKind, MidnightVK)> {
    let (tag, vk_bytes) = blob
        .split_first()
        .ok_or_else(|| anyhow!("empty `verify_proof_vks` entry"))?;
    let kind = DeciderKind::from_tag(*tag)?;
    let vk = MidnightVK::read(&mut { vk_bytes }, SerdeFormat::Processed)
        .map_err(|e| anyhow!("reading inner verifying key: {e}"))?;
    Ok((kind, vk))
}

/// Public-input encoding of a resolved, collapsed accumulator.
pub fn accumulator_pis(acc: &Accumulator<S>) -> Vec<outer::Scalar> {
    <AssignedAccumulator<S> as Instantiable<outer::Scalar>>::as_public_input(acc)
}

/// Encoding of the accumulator a guarded-off `verify_proof` exposes: the
/// trivial one, which satisfies the pairing invariant by construction.
pub fn trivial_accumulator_pis() -> Vec<outer::Scalar> {
    accumulator_pis(&Accumulator::<S>::trivial(&[]))
}

/// The last [`accumulator_pi_len`] entries of an inner proof's instance. 
fn accumulator_tail<T>(instance: &[T]) -> anyhow::Result<&[T]> {
    let acc_len = accumulator_pi_len();
    instance
        .len()
        .checked_sub(acc_len)
        .map(|start| &instance[start..])
        .ok_or_else(|| {
            anyhow!(
                "a `Collapsed` inner proof's instance must end with the {acc_len} fields of a \
                 collapsed accumulator, but it has {}",
                instance.len()
            )
        })
}

/// Reads the accumulator a `Collapsed` inner proof carries in the tail of its
/// instance, and checks it is collapsed.
fn carried_accumulator(instance: &[outer::Scalar]) -> anyhow::Result<Accumulator<S>> {
    let tail = accumulator_tail(instance)?;
    let acc = reconstruct_accumulator(tail).ok_or_else(|| {
        anyhow!(
            "the tail of a `Collapsed` inner proof's instance is not a well-formed collapsed \
             accumulator; producers must normalise with `collapse -> resolve_fixed_bases -> \
             collapse` before exposing it"
        )
    })?;

    // The encoding is `lhs_point || lhs_scalar || rhs_point || rhs_scalar`, so
    // each side's scalar is the last field of its half. A collapsed accumulator
    // has both equal to one, and `assign_collapsed_accumulator` constrains them
    // to one.
    let half = tail.len() / 2;
    if tail[half - 1] != outer::Scalar::ONE || tail[tail.len() - 1] != outer::Scalar::ONE {
        bail!("a carried accumulator is not collapsed: both side scalars must be one");
    }

    Ok(acc)
}

/// Final off-circuit step for a `DeciderKind`: folds in whatever the inner proof
/// carries, then resolves and collapses the result.
///
/// Takes no `guard`, unlike [`decide_incircuit`]: a guarded-off instruction
/// never reaches here, `verify_proof_offcircuit` returns early instead.
pub fn decide_offcircuit(
    kind: DeciderKind,
    own_acc: Accumulator<S>,
    bases: &BTreeMap<String, <S as SelfEmulation>::C>,
    instance: &[outer::Scalar],
) -> anyhow::Result<Accumulator<S>> {
    let mut acc = match kind {
        DeciderKind::None => own_acc,
        DeciderKind::Collapsed => Accumulator::accumulate(&[own_acc, carried_accumulator(instance)?]),
    };

    acc.resolve_fixed_bases(bases);
    acc.collapse();

    Ok(acc)
}

/// In-circuit counterpart of [`decide_offcircuit`], constraining the resulting
/// accumulator as public inputs.
///
/// Takes `guard` because a circuit's shape cannot depend on a witness: where the
/// off-circuit pass returns early, this one folds the guard in instead.
pub fn decide_incircuit(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<outer::Scalar>,
    kind: DeciderKind,
    own_acc: AssignedAccumulator<S>,
    bases: &BTreeMap<String, AssignedPoint>,
    instance: &[AssignedNative<outer::Scalar>],
    guard: &AssignedBit<outer::Scalar>,
) -> Result<(), Error> {
    let verifier = std.verifier();
    let bls = std.bls12_381();
    let scalar_chip = bls.scalar_field_chip();

    // TODO: if we use truncated challenges it may make sense to collapse before
    // accumulating. 
    let mut acc = match kind {
        DeciderKind::None => own_acc,
        DeciderKind::Collapsed => {
            let carried = compute_carried_accumulator(std, layouter, instance, guard)?;
            verifier.accumulate(layouter, &[own_acc, carried])?
        }
    };

    acc.resolve_fixed_bases(bases);

    AssignedAccumulator::scale_by_bit(layouter, scalar_chip, guard, &mut acc)?;
    acc.collapse(layouter, bls, scalar_chip)?;

    verifier.constrain_as_public_input(layouter, &acc)
}

/// Reconstructs, in-circuit, the accumulator an inner proof carries in
/// its instance tail.
fn compute_carried_accumulator(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<outer::Scalar>,
    instance: &[AssignedNative<outer::Scalar>],
    guard: &AssignedBit<outer::Scalar>,
) -> Result<AssignedAccumulator<S>, Error> {
    let tail = accumulator_tail(instance).map_err(|e| Error::Synthesis(e.to_string()))?;

    let mut selected_limbs = Vec::with_capacity(tail.len());
    for (instance, trivial) in tail.iter().zip(trivial_accumulator_pis()) {
        let trivial = std.assign_fixed(layouter, trivial)?;
        selected_limbs.push(std.select(layouter, guard, instance, &trivial)?);
    }

    // All following operations are required when assigning the accumulator. 
    let assigned_accumulator = {
        // NOTE: this could be replaced once we have 'from_public_inputs'
        let accumulator = selected_limbs
            .iter()
            .map(|wire| wire.value().copied())
            .collect::<Value<Vec<_>>>()
            .map_with_result(|fields| {
                reconstruct_accumulator(&fields).ok_or_else(|| {
                    Error::Synthesis("the carried accumulator's encoding is malformed".into())
                })
            })?;

        let assigned_accumulator = std
            .verifier()
            .assign_collapsed_accumulator(layouter, &[], accumulator)?;

        for (selected, assigned) in selected_limbs
            .iter()
            .zip(std.verifier().as_public_input(layouter, &assigned_accumulator)?)
        {
            std.assert_equal(layouter, selected, &assigned)?;
        }
        assigned_accumulator
    };

    Ok(assigned_accumulator)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tag byte is consensus wire format. If this fails because a variant
    /// moved, the fix is to move it back: append instead.
    #[test]
    fn decider_tags_are_stable() {
        assert_eq!(DeciderKind::None.tag(), 0);
        assert_eq!(DeciderKind::Collapsed.tag(), 1);
    }

    #[test]
    fn an_unknown_tag_is_rejected() {
        for byte in [2u8, 3, 255] {
            assert!(
                deserialize_vk(&[byte, 0, 0, 0]).is_err(),
                "tag {byte} must not deserialize"
            );
        }
        assert!(deserialize_vk(&[]).is_err(), "an empty blob must not parse");
    }

    #[test]
    fn a_non_collapsed_tail_is_rejected() {
        let mut tail = trivial_accumulator_pis();
        assert!(carried_accumulator(&tail).is_ok());

        let last = tail.len() - 1;
        tail[last] = outer::Scalar::from(2u64);
        assert!(
            carried_accumulator(&tail).is_err(),
            "a right-hand-side scalar other than one must be rejected"
        );

        assert!(
            carried_accumulator(&[]).is_err(),
            "an instance with no room for a tail must be rejected"
        );
    }
}
