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

//! Fixtures for exercising `verify_proof` from crates that hold no prover of
//! their own.

use crate::decider::{DeciderKind, serialize_vk};
use midnight_circuits::hash::poseidon::PoseidonState;
use midnight_circuits::instructions::{AssignmentInstructions, PublicInputInstructions};
use midnight_circuits::types::AssignedNative;
use midnight_curves::Fq;
use midnight_proofs::circuit::{Layouter, Value};
use midnight_proofs::plonk;
use midnight_zk_stdlib::{Relation, ZkStdLib, ZkStdLibArch, optimal_k, prove, setup_pk, setup_vk};
use rand::{CryptoRng, Rng};
use transient_crypto::curve::Fr;
use transient_crypto::proofs::ParamsProverProvider;

/// Exposes one field element as its only public input, and says nothing else.
#[derive(Clone)]
struct Echo;

impl Relation for Echo {
    type Instance = Fq;
    type Witness = ();
    type Error = plonk::Error;

    fn format_instance(instance: &Fq) -> Result<Vec<Fq>, plonk::Error> {
        Ok(vec![*instance])
    }

    fn circuit(
        &self,
        std_lib: &ZkStdLib,
        layouter: &mut impl Layouter<Fq>,
        instance: Value<Fq>,
        _witness: Value<()>,
    ) -> Result<(), plonk::Error> {
        let x: AssignedNative<Fq> = std_lib.assign(layouter, instance)?;
        std_lib.constrain_as_public_input(layouter, &x)
    }

    fn used_chips(&self) -> ZkStdLibArch {
        ZkStdLibArch::default()
    }

    fn write_relation<W: std::io::Write>(&self, _writer: &mut W) -> std::io::Result<()> {
        Ok(())
    }

    fn read_relation<R: std::io::Read>(_reader: &mut R) -> std::io::Result<Self> {
        Ok(Echo)
    }
}

/// An inner proof, with what a circuit needs in order to verify it.
pub struct InnerProof {
    /// The [`IrSource::verify_proof_vks`](crate::IrSource::verify_proof_vks)
    /// entry the verifying `VerifyProof` names by its hash.
    pub vk_blob: Vec<u8>,
    /// The proof itself, as an
    /// [`InnerProofWitness`](transient_crypto::proofs::InnerProofWitness).
    pub proof: Vec<u8>,
    /// The public inputs the verifying circuit passes as `instance`.
    pub instance: Vec<Fr>,
}

/// Proves that `value` is `value`, with the transcript `verify_proof` reads.
///
/// A proof from the ZKIR pipeline will not do: it is written to a
/// [`TranscriptHash`](transient_crypto::proofs::TranscriptHash) — Blake2b —
/// whereas the in-circuit verifier reads a Poseidon one, which is why this
/// lives here rather than in the calling crate's test.
///
/// The relation defers no accumulator of its own, so the entry is tagged
/// [`DeciderKind::None`].
pub async fn echo_proof(
    params: &impl ParamsProverProvider,
    value: Fr,
    rng: impl Rng + CryptoRng,
) -> anyhow::Result<InnerProof> {
    let srs = params.get_params(optimal_k(&Echo) as u8).await?;
    let vk = setup_vk(srs.as_ref(), &Echo);
    let pk = setup_pk(&Echo, &vk);
    let proof = prove::<Echo, PoseidonState<Fq>>(srs.as_ref(), &pk, &Echo, &value.0, (), rng)?;
    Ok(InnerProof {
        vk_blob: serialize_vk(&vk, DeciderKind::None)?,
        proof,
        instance: vec![value],
    })
}
