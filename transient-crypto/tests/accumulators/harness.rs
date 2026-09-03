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

//! Scaffolding for the accumulator tests: the verify side — rebuilding the
//! accumulators a proof carries and running their deferred pairing.
//!
//! This crate cannot build a *real* accumulator: those come from ZKIR's
//! `verify_proof`, and `zkir-v3` depends on this crate, not the other way
//! round. What it can do is exploit how `verify` assembles the public-input
//! vector — `proof.accumulators` flattened, then the caller's statement — and
//! prove exactly that vector with [`ExposeAll`]. Handing back the head as
//! accumulator blocks and the tail as the statement then drives extraction,
//! reconstruction and pairing through the public `verify` API.
//!
//! So these tests cover this layer's plumbing, not end-to-end recursion. That a
//! genuine `verify_proof` accumulator survives the same path is `zkir-v3`'s
//! `verify_proof` suite.

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
use midnight_transient_crypto::proofs::{
    InnerSelfEmulation as S, ParamsProver, Proof, TranscriptHash, VerifierKey, accumulator_pi_len,
};

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

/// Public-input width of one fully-collapsed accumulator.
pub fn acc_len() -> usize {
    accumulator_pi_len()
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

/// Proves the public-input vector `accs` flattened followed by `tail`, and
/// hands the pieces back the way [`VerifierKey::verify`] expects them: the
/// blocks on the `Proof`, only `tail` as the caller-facing statement.
///
/// An empty `accs` gives a proof carrying no accumulators at all.
pub fn proof_carrying(
    accs: &[Vec<Fq>],
    tail: &[Fq],
    rng: &mut ChaCha20Rng,
) -> (VerifierKey, Proof, Vec<Fr>) {
    let mut pis: Vec<Fq> = accs.concat();
    pis.extend_from_slice(tail);
    let (vk, bytes) = raw_proof_exposing(&pis, rng);
    (
        VerifierKey::from(vk),
        Proof {
            bytes,
            accumulators: accs.iter().map(|a| fr_vec(a)).collect(),
        },
        fr_vec(tail),
    )
}

/// Field elements as the `Fr` newtype the proof API speaks in.
pub fn fr_vec(fields: &[Fq]) -> Vec<Fr> {
    fields.iter().copied().map(Fr).collect()
}

/// As [`proof_carrying`], but hands back the raw `MidnightVK` and proof bytes
/// so a caller can split the public inputs into blocks and statement however it
/// likes — including ways no honest prover would.
pub fn raw_proof_exposing(
    pis: &[Fq],
    rng: &mut ChaCha20Rng,
) -> (midnight_zk_stdlib::MidnightVK, Vec<u8>) {
    let relation = ExposeAll(pis.len());
    let k = (optimal_k(&relation) as u8).max(MIN_SRS_K);
    let params = srs(k);
    let vk = setup_vk(params.as_ref(), &relation);
    let pk = setup_pk(&relation, &vk);
    let bytes =
        prove::<ExposeAll, TranscriptHash>(params.as_ref(), &pk, &relation, &pis.to_vec(), (), rng)
            .expect("prove");
    (vk, bytes)
}

/// Floor on the SRS size, since `optimal_k` can report below the smallest
/// params file on disk.
pub const MIN_SRS_K: u8 = 10;
