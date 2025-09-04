// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
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

#![cfg(feature = "mock-verify")]
//! ONLY FOR USE DURING BENCHMARKING OR TESTING
//!
//! Allows for realistic-ish system benchmarks without having to generate proofs
//!
//! Specifically, we only generate proofs at calibration-time, so that later we can use the
//! verification timings of those proofs to produce an interpolated mock for benchmarks
//!
//! Note that for realistic benchmarking, the calibration should be run with the `--release` flag,
//! otherwise the conditions under which the measurements are taken won't be representative
use crate::curve::{Fr, outer};
use crate::proofs::{
    KeyLocation, PARAMS_VERIFIER, ParamsProver, ParamsProverProvider, Proof, ProofPreimage,
    ProverKey, ProvingError, ProvingKeyMaterial, Resolver, TranscriptHash, VerifierKey,
    VerifyingError, Zkir,
};
use futures::executor::block_on;
use midnight_circuits::{
    compact_std_lib::Relation,
    instructions::{AssignmentInstructions, PublicInputInstructions},
    types::AssignedNative,
};
use rand::Rng;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use serialize::{Deserializable, Serializable, Tagged, tagged_serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::fs;
use std::hint::black_box;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;
use std::{fs::File, io::Read, path::Path};

#[derive(Debug, Serialize, Deserialize, Clone)]
/// Represents performance of verification of a proof
///
/// `median_iters` is the number of SHA-256 burn iterations that
/// approximates the median time real verification takes for `circuit_inputs` inputs
pub struct CalibrationRecord {
    /// Number of public inputs for the circuit
    pub circuit_inputs: usize,
    /// The number of SHA-256 burn iterations that approximates the median time
    /// real verification takes
    pub median_iters: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CalibrationFile(Vec<CalibrationRecord>);

#[derive(Debug, Clone)]
/// Model of verification performance for a number of proofs
pub struct Calibration {
    /// The number of public inputs of each proof and its performance impact during verification
    pub points: Vec<CalibrationRecord>,
}

impl Calibration {
    fn load<P: AsRef<Path>>(p: P) -> std::io::Result<Self> {
        let mut s = String::new();
        File::open(p)?.read_to_string(&mut s)?;
        let mut file: CalibrationFile = serde_json::from_str(&s)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        file.0.sort_by_key(|r| r.circuit_inputs);
        Ok(Self { points: file.0 })
    }

    // Find the indices of the two calibration run `circuit_inputs` points that bracket a given
    // number of public inputs
    //
    // So if, in our calibration run, we end up with `[(2, x), (8, y), (24, z)]`, and `neighbours`
    // is passed `6`, it'll return `(0, 1)`, the indices of the element with `circuit_inputs == 2`
    // and the element with `circuit_inputs == 8`
    fn neighbours(&self, n: usize) -> (usize, usize) {
        assert!(!self.points.is_empty(), "points must not be empty");
        let len = self.points.len();

        match self.points.binary_search_by_key(&n, |r| r.circuit_inputs) {
            Ok(i) => (i, i),
            Err(i) => {
                let lo = i.saturating_sub(1);
                let hi = i.min(len - 1);
                (lo, hi)
            }
        }
    }

    // Estimates the number of iterations required to match the time-taken of proof verification for a
    // given number of public inputs by interpolating a point between the two nearest data points
    // observed during calibration
    //
    // The shape of the generated data (at least on my machine, in release mode) is logarithmic,
    // so interpolation needs to be logarithmic, too
    pub(crate) fn median_iters(&self, n: usize) -> u64 {
        let (lo, hi) = self.neighbours(n);
        if lo == hi {
            return self.points[lo].median_iters;
        }
        let (n0, m0) = (
            self.points[lo].circuit_inputs as f64,
            self.points[lo].median_iters as f64,
        );
        let (n1, m1) = (
            self.points[hi].circuit_inputs as f64,
            self.points[hi].median_iters as f64,
        );
        let t = ((n as f64).ln() - n0.ln()) / (n1.ln() - n0.ln());
        (m0 + t * (m1 - m0)).round() as u64
    }
}

// Global `Calibration`, initialised once from the calibration file
static CALIBRATION: OnceLock<Calibration> = OnceLock::new();

fn load_default_calib() -> Calibration {
    let path = calibration_path();
    println!("mock-verify: loading calibration from {}", path.display());
    match Calibration::load(&path) {
        Ok(c) => c,
        Err(_) => calibrate_for(path),
    }
}

fn calibration() -> &'static Calibration {
    CALIBRATION.get_or_init(|| load_default_calib())
}

fn burn_step(i: u64) {
    let mut sha = Sha256::new();
    sha.update(&i.to_be_bytes());
    black_box(sha.finalize());
}

fn dummy_verify(calibrated_iterations: u64) -> Result<(), VerifyingError> {
    for i in 0..calibrated_iterations {
        burn_step(i);
    }
    Ok(())
}

/// Simulates proof verification for a given public-input length using the global calibration
pub fn mock_verify_for(public_input_len: usize) -> Result<(), VerifyingError> {
    dummy_verify(iters_for_len(public_input_len))
}

/// Computes the simulated iteration count for a given public-input length
pub fn iters_for_len(public_input_len: usize) -> u64 {
    calibration().median_iters(public_input_len)
}

/// Run calibration required for proof verification mocks
///
/// Generates and verifies a sample of real proofs
/// at several public-input sizes, then writes to `<crate>/target/calibration.json`.
/// Returns the in-memory `Calibration`.
pub fn calibrate() -> Calibration {
    calibrate_for(calibration_path())
}

/// Run calibration required for proof verification mocks
///
/// Generates and verifies a sample of real proofs
/// at several public-input sizes, then writes to the provided path.
/// Returns the in-memory `Calibration`.
pub fn calibrate_for(path: PathBuf) -> Calibration {
    if cfg!(debug_assertions) {
        println!();
        println!("Warning: Calibrating in debug mode!");
        println!(
            "For representative benchmark timings, please run this with the `--release` flag."
        );
        println!("Example: `cargo run --release --example calibrate`");
        println!();
    }

    struct TestResolver {
        pk: ProverKey<TestIr>,
        vk: VerifierKey,
        ir: TestIr,
    }

    impl Resolver for TestResolver {
        async fn resolve_key(
            &self,
            _key: KeyLocation,
        ) -> std::io::Result<Option<ProvingKeyMaterial>> {
            let mut pk = Vec::new();
            tagged_serialize(&self.pk, &mut pk)?;
            let mut vk = Vec::new();
            tagged_serialize(&self.vk, &mut vk)?;
            let mut ir = Vec::new();
            tagged_serialize(&self.ir, &mut ir)?;
            Ok(Some(ProvingKeyMaterial {
                prover_key: pk,
                verifier_key: vk,
                ir_source: ir,
            }))
        }
    }

    struct TestParams;
    impl ParamsProverProvider for TestParams {
        async fn get_params(&self, k: u8) -> std::io::Result<ParamsProver> {
            const DIR: &str = env!("MIDNIGHT_PP");
            ParamsProver::read(BufReader::new(File::open(format!(
                "{DIR}/bls_filecoin_2p{k}"
            ))?))
        }
    }

    const CALIBRATION_SAMPLES: usize = 50;
    const CPU_BURN_ITERS: u64 = 10_000;
    const PUBLIC_INPUT_COUNTS: &[usize] = &[
        1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192,
    ];

    println!("--- Starting Verification Group Calibration ---");

    let mut calibration_results = Vec::new();

    for &size in PUBLIC_INPUT_COUNTS {
        let ir = TestIr {
            no_inputs: size as u64,
        };

        let (pk, vk) = block_on(ir.keygen(&TestParams)).unwrap();

        let mut rng = OsRng;
        let inp = (0..size).map(|_| rng.r#gen()).collect::<Vec<_>>();
        let preimage = ProofPreimage {
            binding_input: 42.into(),
            communications_commitment: None,
            inputs: inp.clone(),
            private_transcript: vec![],
            public_transcript_inputs: inp.clone(),
            public_transcript_outputs: vec![],
            key_location: KeyLocation(Cow::Borrowed("builtin")),
        };

        let (proof, _) = block_on(preimage.prove::<TestIr>(
            &mut rng,
            &TestParams,
            &TestResolver {
                pk: pk.clone(),
                vk: vk.clone(),
                ir: ir.clone(),
            },
        ))
        .unwrap();

        // Warm up
        let _ = vk.verify(&PARAMS_VERIFIER, &proof, inp.iter().copied());

        let mut samples = Vec::with_capacity(CALIBRATION_SAMPLES);
        for _ in 0..CALIBRATION_SAMPLES {
            let t0 = Instant::now();
            vk.verify(&PARAMS_VERIFIER, &proof, inp.iter().copied())
                .unwrap();
            samples.push(t0.elapsed());
        }
        samples.sort_unstable();

        // Warm up
        let _ = dummy_verify(100);

        let t0 = Instant::now();
        dummy_verify(CPU_BURN_ITERS).unwrap();
        let dummy_ns_per_iter = t0.elapsed().as_nanos() as f64 / CPU_BURN_ITERS as f64;

        let iter_curve: Vec<u64> = samples
            .iter()
            .map(|d| (d.as_nanos() as f64 / dummy_ns_per_iter).round() as u64)
            .collect();

        let median_iters = iter_curve[iter_curve.len() / 2];

        calibration_results.push(CalibrationRecord {
            circuit_inputs: size as usize,
            median_iters,
        });
    }

    let calibration_json =
        serde_json::to_string_pretty(&CalibrationFile(calibration_results.clone()))
            .expect("serialising calibration data");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("creating target/ directory");
    }

    fs::write(&path, calibration_json).expect("writing calibration file");
    println!("✔  calibration file written to {}", path.display());
    Calibration {
        points: calibration_results,
    }
}

fn calibration_path() -> PathBuf {
    if let Ok(path_str) = std::env::var("PROOF_CALIB_PATH") {
        return PathBuf::from(path_str);
    }

    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("calibration.json")
}

#[derive(Clone, Serializable, Debug)]
#[tag = "test-ir"]
struct TestIr {
    no_inputs: u64,
}

impl Relation for TestIr {
    type Instance = Vec<Fr>;
    type Witness = Self;
    fn format_instance(instance: &Self::Instance) -> Vec<outer::Scalar> {
        instance.iter().map(|x| x.0).collect()
    }
    fn write_relation<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        self.serialize(writer)
    }
    fn read_relation<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        Self::deserialize(reader, 0)
    }
    fn circuit(
        &self,
        std_lib: &midnight_circuits::compact_std_lib::ZkStdLib,
        layouter: &mut impl midnight_proofs::circuit::Layouter<outer::Scalar>,
        instance: midnight_proofs::circuit::Value<Self::Instance>,
        _witness: midnight_proofs::circuit::Value<Self::Witness>,
    ) -> Result<(), midnight_proofs::plonk::Error> {
        for i in 0..self.no_inputs {
            let value = instance.as_ref().map(|v| v[i as usize].0);
            let cell: AssignedNative<outer::Scalar> = std_lib.assign(layouter, value)?;
            std_lib.constrain_as_public_input(layouter, &cell)?;
        }
        Ok(())
    }
}

impl Zkir for TestIr {
    fn check(&self, _preimage: &ProofPreimage) -> Result<Vec<Option<usize>>, ProvingError> {
        Ok(vec![])
    }
    async fn prove(
        &self,
        rng: impl Rng + rand::CryptoRng,
        params: &impl ParamsProverProvider,
        pk: ProverKey<Self>,
        preimage: &ProofPreimage,
    ) -> Result<(Proof, Vec<Fr>, Vec<Option<usize>>), ProvingError> {
        use midnight_circuits::compact_std_lib::prove;
        let params_k = params.get_params(pk.init()?.k()).await?;
        let pis = preimage.public_transcript_inputs.clone();
        let pk = pk.init().unwrap();
        let proof =
            prove::<_, TranscriptHash>(params_k.as_ref(), &pk, self, &pis, self.clone(), rng)?;
        Ok((
            Proof(proof),
            preimage.public_transcript_inputs.clone(),
            vec![],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_calibration() -> Calibration {
        let points = vec![
            CalibrationRecord {
                circuit_inputs: 4,
                median_iters: 100,
            },
            CalibrationRecord {
                circuit_inputs: 16,
                median_iters: 400,
            },
        ];
        Calibration { points }
    }

    #[test]
    fn median_is_interpolated_log() {
        let _ = CALIBRATION.set(test_calibration());

        let iters_8 = iters_for_len(8);
        assert!(iters_8 == 250, "iters_8={iters_8}");

        let iters_12 = iters_for_len(12);
        assert!(iters_12 == 338, "iters_12={iters_12}");
    }

    #[test]
    fn median_is_interpolated_log_exact() {
        let _ = CALIBRATION.set(test_calibration());

        let iters_4 = iters_for_len(4);
        assert!(iters_4 == 100, "iters_4={iters_4}");

        let iters_16 = iters_for_len(16);
        assert!(iters_16 == 400, "iters_16={iters_16}");
    }

    #[test]
    fn mock_verify_for_executes_successfully() {
        let _ = CALIBRATION.set(test_calibration());
        mock_verify_for(8).expect("mock verify should succeed");
    }
}
