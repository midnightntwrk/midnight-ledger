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

//! Shared setup for the accumulator tests.
//!
//! Real accumulators come from `zkir-v3`'s `verify_proof`, and this crate
//! cannot depend on `zkir-v3`. Instead, [`ExposeAll`] proves any public-input
//! vector, and the tests carry its head as accumulators and pass its tail as
//! the statement, the layout `verify` rebuilds. That covers this crate's verify
//! path; real recursion is covered by `zkir-v3`'s `verify_proof` suite.

use std::collections::BTreeMap;
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

use midnight_transient_crypto::curve::Fr;
use midnight_transient_crypto::proofs::{
    DeferredAccumulator, InnerSelfEmulation as S, ParamsProver, Proof, TranscriptHash, VerifierKey,
};

/// Fixed seed, so runs are reproducible.
pub fn test_rng() -> ChaCha20Rng {
    ChaCha20Rng::from_seed([7; 32])
}

/// Reads `bls_midnight_2p{k}` from `$MIDNIGHT_PP`, or
/// `~/.cache/midnight/zk-params`.
fn srs(k: u8) -> ParamsProver {
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = std::env::var("MIDNIGHT_PP").unwrap_or(format!("{home}/.cache/midnight/zk-params"));
    let path = format!("{dir}/bls_midnight_2p{k}");
    let file = File::open(&path).unwrap_or_else(|e| {
        panic!("cannot open SRS params at {path} ({e}); point MIDNIGHT_PP at a directory of bls_midnight_2p<k> files")
    });
    ParamsProver::read(BufReader::new(file))
        .unwrap_or_else(|e| panic!("cannot read SRS params at {path}: {e}"))
}

fn encode(acc: &Accumulator<S>) -> Vec<Fq> {
    <AssignedAccumulator<S> as Instantiable<Fq>>::as_public_input(acc)
}

/// An encoded accumulator that pairs: both sides are the identity.
pub fn passing_accumulator() -> Vec<Fq> {
    encode(&Accumulator::<S>::trivial(&[]))
}

/// An encoded accumulator that decodes but does not pair: `lhs` is the
/// generator, `rhs` the identity.
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

/// Exposes its whole instance as public inputs, so a test controls exactly
/// what the proof commits to.
#[derive(Clone, Default)]
struct ExposeAll(usize);

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

/// Proves `accs ++ tail`, carrying `accs` on the `Proof` and returning `tail`
/// as the statement.
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
            accumulators: accs.iter().map(|a| deferred(a)).collect(),
        },
        fr_vec(tail),
    )
}

/// An encoded accumulator as a [`DeferredAccumulator`].
pub fn deferred(fields: &[Fq]) -> DeferredAccumulator {
    DeferredAccumulator::from_public_input(fields)
        .expect("test accumulator must be collapsed and fixed-base-resolved")
}

/// Converts to the `Fr` newtype the proof API takes.
fn fr_vec(fields: &[Fq]) -> Vec<Fr> {
    fields.iter().copied().map(Fr).collect()
}

/// Proves `pis`, returning the raw VK and proof bytes.
fn raw_proof_exposing(
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

/// The smallest params file on disk; `optimal_k` can go lower.
const MIN_SRS_K: u8 = 10;
