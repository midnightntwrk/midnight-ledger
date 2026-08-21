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

//! Scaffolding for the accumulator tests: the verify side — reconstructing
//! accumulators from public inputs at their recorded offsets and running the
//! deferred pairing.
//!
//! This crate cannot build a *real* accumulator: those come from ZKIR's
//! `verify_proof`, and `zkir-v3` depends on this crate, not the other way
//! round. What it can do is put a *valid encoding* of one in a proof's public
//! inputs — [`ExposeAll`] proves a statement that is literally the encoding —
//! which is enough to drive extraction, reconstruction and pairing through the
//! public `verify` API.
//!
//! So these tests cover this layer's plumbing, not end-to-end recursion. That a
//! genuine `verify_proof` accumulator survives the same path is case 3.2.

use std::fs::File;
use std::io::BufReader;

use group::Group;
use midnight_circuits::instructions::{AssignmentInstructions, PublicInputInstructions};
use midnight_circuits::types::{AssignedNative, Instantiable};
use midnight_circuits::verifier::{Accumulator, AssignedAccumulator, Msm, SelfEmulation};
use midnight_curves::Fq;
use midnight_proofs::circuit::{Layouter, Value};
use midnight_proofs::plonk;
use midnight_zk_stdlib::{Relation, ZkStdLib, ZkStdLibArch, optimal_k, prove, setup_pk, setup_vk};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use std::collections::BTreeMap;

use midnight_transient_crypto::curve::Fr;
use midnight_transient_crypto::proofs::{ParamsProver, Proof, S, TranscriptHash, VerifierKey};

/// A deterministic RNG, so every test here is reproducible.
pub fn test_rng() -> ChaCha20Rng {
    ChaCha20Rng::from_seed([7; 32])
}

/// SRS params read at run time from `$MIDNIGHT_PP`, falling back to
/// `~/.cache/midnight/zk-params`.
pub fn srs(k: u8) -> ParamsProver {
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = std::env::var("MIDNIGHT_PP").unwrap_or(format!("{home}/.cache/midnight/zk-params"));
    ParamsProver::read(BufReader::new(
        File::open(format!("{dir}/bls_midnight_2p{k}")).expect("SRS params"),
    ))
    .expect("read SRS")
}

/// Public-input width of one fully-collapsed accumulator. Mirrors the crate's
/// own private `accumulator_pi_len`.
pub fn acc_len() -> usize {
    encode(&Accumulator::<S>::trivial(&[])).len()
}

fn encode(acc: &Accumulator<S>) -> Vec<Fq> {
    <AssignedAccumulator<S> as Instantiable<Fq>>::as_public_input(acc)
}

/// The encoding of an accumulator whose deferred pairing passes: both sides are
/// the identity, so both pairings are trivially equal.
pub fn passing_accumulator() -> Vec<Fq> {
    encode(&Accumulator::<S>::trivial(&[]))
}

/// The encoding of an accumulator that decodes cleanly but does *not* pair:
/// `lhs` is the generator where `rhs` is the identity. Well-formed field
/// elements throughout, so only the pairing check can refuse it.
pub fn failing_accumulator() -> Vec<Fq> {
    let one = <S as SelfEmulation>::F::from(1u64);
    encode(&Accumulator::<S>::new(
        Msm::new(
            &[<S as SelfEmulation>::C::generator()],
            &[one],
            &BTreeMap::new(),
        ),
        Msm::new(
            &[<S as SelfEmulation>::C::identity()],
            &[one],
            &BTreeMap::new(),
        ),
    ))
}

/// Exposes every element of its instance as a public input, verbatim. Lets a
/// test choose exactly what the proof's public-input vector contains.
#[derive(Clone, Default)]
pub struct ExposeAll(pub usize);

impl Relation for ExposeAll {
    type Instance = Vec<Fq>;
    type Witness = ();
    type Error = plonk::Error;

    fn format_instance(instance: &Vec<Fq>) -> Result<Vec<Fq>, plonk::Error> {
        Ok(instance.clone())
    }

    fn circuit(
        &self,
        std: &ZkStdLib,
        layouter: &mut impl Layouter<Fq>,
        instance: Value<Vec<Fq>>,
        _witness: Value<()>,
    ) -> Result<(), plonk::Error> {
        for i in 0..self.0 {
            let v: AssignedNative<Fq> = std.assign(layouter, instance.as_ref().map(|x| x[i]))?;
            std.constrain_as_public_input(layouter, &v)?;
        }
        Ok(())
    }

    fn used_chips(&self) -> ZkStdLibArch {
        ZkStdLibArch::default()
    }

    fn write_relation<W: std::io::Write>(&self, w: &mut W) -> std::io::Result<()> {
        w.write_all(&(self.0 as u32).to_le_bytes())
    }

    fn read_relation<R: std::io::Read>(r: &mut R) -> std::io::Result<Self> {
        let mut b = [0u8; 4];
        r.read_exact(&mut b)?;
        Ok(ExposeAll(u32::from_le_bytes(b) as usize))
    }
}

/// Proves that the public-input vector is exactly `pis`, and returns a
/// `VerifierKey` recording `offsets` alongside the proof and statement.
pub fn proof_exposing(
    pis: &[Fq],
    offsets: &[usize],
    rng: &mut ChaCha20Rng,
) -> (VerifierKey, Proof, Vec<Fr>) {
    let (vk, proof) = raw_proof_exposing(pis, rng);
    (
        VerifierKey::from_vk_with_accumulator_offsets(vk, offsets),
        proof,
        pis.iter().map(|f| Fr(*f)).collect(),
    )
}

/// As [`proof_exposing`], handing back the raw `MidnightVK` so a caller can
/// record its own offsets — or none at all.
pub fn raw_proof_exposing(
    pis: &[Fq],
    rng: &mut ChaCha20Rng,
) -> (midnight_zk_stdlib::MidnightVK, Proof) {
    let relation = ExposeAll(pis.len());
    let k = (optimal_k(&relation) as u8).max(MIN_SRS_K);
    let params = srs(k);
    let vk = setup_vk(params.as_ref(), &relation);
    let pk = setup_pk(&relation, &vk);
    let bytes =
        prove::<ExposeAll, TranscriptHash>(params.as_ref(), &pk, &relation, &pis.to_vec(), (), rng)
            .expect("prove");
    (vk, Proof(bytes))
}

/// Floor on the SRS size, since `optimal_k` can report below the smallest
/// params file on disk.
pub const MIN_SRS_K: u8 = 10;
