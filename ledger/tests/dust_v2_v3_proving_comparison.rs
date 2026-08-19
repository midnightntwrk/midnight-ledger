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

//! Compares the Dust spend circuit as shipped for the old "V2" proving
//! pipeline (verifier-key\[v6\], fixture preserved under `tests/dust_v2/`)
//! against the current "V3" pipeline (verifier-key\[v7\], `static/dust/`),
//! on: proof generation time, proof size, and verifying key size.
//!
//! Both circuits are exercised with the *same* witness, built from a real
//! `DustSpend` produced by the current ledger code (`DustLocalState::spend`)
//! - i.e. the same public-input/private-witness shape that production code
//! builds today. That is only a fair comparison because both circuits accept
//! that exact preimage; this is asserted below via `Zkir::check` before
//! timing anything.
//!
//! The two fixtures turn out to require two different proving pipelines:
//! - `static/dust/spend.bzkir` is tagged `ir-source[v3-generic]` and decodes
//!   as a `zkir_v3::IrSource`, proved through the current
//!   `transient_crypto::proofs::Zkir` pipeline.
//! - `tests/dust_v2/spend.bzkir` is tagged `ir-source[v2]` (the legacy,
//!   pre-"generic" container format), which `zkir_v2::IrSource::load_from_tagged`
//!   loads as `IrMinorVersion::V0` - the oldest supported circuit generation,
//!   which must be proved through `transient_crypto_old::proofs::Zkir`
//!   (`midnight-transient-crypto` 2.x) rather than the current crate. This
//!   matches its on-disk verifier key tag, `verifier-key[v6]` (vs. `[v7]`
//!   for V3). `zkir_v2::ir_v1` bridges the two: `preimage_to_v1` downgrades
//!   the (shared) preimage, and `V1Params` adapts a current
//!   `ParamsProverProvider` into an old one.
//!
//! This intentionally avoids the production key-fetching path
//! (`DustResolver`/`MidnightDataProvider`), which downloads the prover key
//! and trusted-setup parameters from `https://srs.midnight.network` and
//! requires them to already be cached locally. Instead, this test performs
//! its own key generation (`Zkir::keygen`) against a throwaway, locally
//! generated (i.e. *not* production-trustworthy) KZG SRS. Proof size,
//! verifying-key size and proving time depend only on the circuit's `k`, not
//! on which SRS backs it, so this is representative of production timings
//! while remaining fully offline and deterministic.
//!
//! Run with: `cargo test -p midnight-ledger-v9 --features proving --test dust_v2_v3_proving_comparison -- --nocapture`

#![cfg(feature = "proving")]

use std::sync::Arc;
use std::time::{Duration, Instant};

use midnight_curves::Bls12;
use midnight_ledger::test_utilities::TestState;
use midnight_ledger_v9 as midnight_ledger;
use midnight_proofs::poly::kzg::params::ParamsKZG;
use rand::{SeedableRng, rngs::StdRng};
use serialize::tagged_serialize;
use storage::db::InMemoryDB;
use transient_crypto::proofs::{ParamsProver, ParamsProverProvider};

/// The current ("V3") Dust spend circuit, as shipped in production.
const V3_BZKIR: &[u8] = include_bytes!("../static/dust/spend.bzkir");
/// The previous ("V2") Dust spend circuit, preserved as a regression fixture.
const V2_BZKIR: &[u8] = include_bytes!("dust_v2/spend.bzkir");

/// A `ParamsProverProvider` backed by a throwaway, in-memory KZG setup.
///
/// Real proving needs public parameters ("SRS") sized to the circuit's `k`.
/// Production fetches these (and the matching prover/verifier keys) from a
/// network data provider. Since proof/key sizes and proving time do not
/// depend on *which* SRS is used (only on the circuit's `k`), generating our
/// own insecure SRS locally is a faithful stand-in for benchmarking purposes,
/// and keeps this test hermetic.
struct LocalUnsafeParams;

impl ParamsProverProvider for LocalUnsafeParams {
    async fn get_params(&self, k: u8) -> std::io::Result<ParamsProver> {
        let mut rng = StdRng::seed_from_u64(0x5eed_0000 ^ k as u64);
        Ok(ParamsProver(Arc::new(ParamsKZG::<Bls12>::unsafe_setup(
            k as u32, &mut rng,
        ))))
    }
}

struct Measurement {
    label: &'static str,
    k: u8,
    keygen_time: Duration,
    prove_time: Duration,
    proof_size: usize,
    vk_size: usize,
}

/// Serializes the same throwaway SRS that `LocalUnsafeParams::get_params`
/// hands out for `k`, so a matching `ParamsVerifier` can be derived for
/// self-verification. The real, checked-in `PARAMS_VERIFIER` static in each
/// crate belongs to the *production* trusted setup, which our proofs are
/// deliberately not made against (this test is fully offline), so
/// self-verification must instead check against a verifier key derived from
/// the same local SRS used for proving.
async fn local_verifier_params_bytes(k: u8) -> Vec<u8> {
    let prover = LocalUnsafeParams
        .get_params(k)
        .await
        .expect("local params generation should succeed");
    let mut buf = Vec::new();
    prover
        .0
        .write_custom(
            &mut buf,
            midnight_proofs::utils::SerdeFormat::RawBytesUnchecked,
        )
        .expect("local params should serialize");
    buf
}

/// Measures the current ("V3") pipeline.
async fn measure_v3(
    ir: zkir_v3::IrSource,
    preimage: &transient_crypto::proofs::ProofPreimage,
) -> Measurement {
    use transient_crypto::proofs::Zkir;

    let params = LocalUnsafeParams;
    let k = ir.k();

    let t0 = Instant::now();
    let (pk, vk) = ir
        .keygen(&params)
        .await
        .unwrap_or_else(|e| panic!("dust_v3: keygen failed: {e}"));
    let keygen_time = t0.elapsed();

    let mut vk_bytes = Vec::new();
    tagged_serialize(&vk, &mut vk_bytes).expect("verifier key should serialize");

    let rng = StdRng::seed_from_u64(0x4242);
    let t1 = Instant::now();
    let (proof, pis, _skips) = ir
        .prove(rng, &params, pk, preimage)
        .await
        .unwrap_or_else(|e| panic!("dust_v3: proving failed: {e}"));
    let prove_time = t1.elapsed();

    let vparams_bytes = local_verifier_params_bytes(k).await;
    let vparams = transient_crypto::proofs::ParamsVerifier::read(&vparams_bytes[..])
        .expect("verifier params should read back");
    vk.verify(&vparams, &proof, pis.into_iter())
        .unwrap_or_else(|e| panic!("dust_v3: self-verification of the produced proof failed: {e}"));

    Measurement {
        label: "dust_v3",
        k,
        keygen_time,
        prove_time,
        proof_size: proof.0.len(),
        vk_size: vk_bytes.len(),
    }
}

/// Measures the legacy ("V2") pipeline. `tests/dust_v2/spend.bzkir` decodes
/// to `IrMinorVersion::V0`, which only the *old* `transient_crypto_old`
/// generation of the proving stack knows how to prove; `zkir_v2::ir_v1`
/// provides the glue (preimage/params conversion) to reuse the same
/// `preimage` and `LocalUnsafeParams` from the V3 side above.
async fn measure_v2(
    ir: zkir_v2::IrSource,
    preimage: &transient_crypto::proofs::ProofPreimage,
) -> Measurement {
    use transient_crypto_old::proofs::Zkir as OldZkir;
    use zkir_v2::ir_v1::{V1Params, preimage_to_v1};

    let params = LocalUnsafeParams;
    let old_params = V1Params(&params);
    let old_preimage = preimage_to_v1(preimage);

    let k = OldZkir::k(&ir);

    let t0 = Instant::now();
    let (pk, vk) = OldZkir::keygen(&ir, &old_params)
        .await
        .unwrap_or_else(|e| panic!("dust_v2: keygen failed: {e}"));
    let keygen_time = t0.elapsed();

    let mut vk_bytes = Vec::new();
    tagged_serialize(&vk, &mut vk_bytes).expect("verifier key should serialize");

    let rng = StdRng::seed_from_u64(0x4242);
    let t1 = Instant::now();
    let (proof, pis, _skips) = OldZkir::prove(&ir, rng, &old_params, pk, &old_preimage)
        .await
        .unwrap_or_else(|e| panic!("dust_v2: proving failed: {e}"));
    let prove_time = t1.elapsed();

    let vparams_bytes = local_verifier_params_bytes(k).await;
    let vparams = transient_crypto_old::proofs::ParamsVerifier::read(&vparams_bytes[..])
        .expect("verifier params should read back");
    vk.verify(&vparams, &proof, pis.into_iter())
        .unwrap_or_else(|e| panic!("dust_v2: self-verification of the produced proof failed: {e}"));

    Measurement {
        label: "dust_v2",
        k,
        keygen_time,
        prove_time,
        proof_size: proof.0.len(),
        vk_size: vk_bytes.len(),
    }
}

fn print_report(v2: &Measurement, v3: &Measurement) {
    println!();
    println!(
        "{:<10} {:>4} {:>14} {:>14} {:>12} {:>10}",
        "circuit", "k", "keygen", "prove", "proof (B)", "vk (B)"
    );
    for m in [v2, v3] {
        println!(
            "{:<10} {:>4} {:>14.3?} {:>14.3?} {:>12} {:>10}",
            m.label, m.k, m.keygen_time, m.prove_time, m.proof_size, m.vk_size
        );
    }
    let pct = |old: f64, new: f64| (new - old) / old * 100.0;
    println!();
    println!(
        "proof generation time: V3 is {:+.1}% vs V2 ({:.3?} -> {:.3?})",
        pct(v2.prove_time.as_secs_f64(), v3.prove_time.as_secs_f64()),
        v2.prove_time,
        v3.prove_time,
    );
    println!(
        "proof size:            V3 is {:+.1}% vs V2 ({} B -> {} B)",
        pct(v2.proof_size as f64, v3.proof_size as f64),
        v2.proof_size,
        v3.proof_size,
    );
    println!(
        "verifying key size:    V3 is {:+.1}% vs V2 ({} B -> {} B)",
        pct(v2.vk_size as f64, v3.vk_size as f64),
        v2.vk_size,
        v3.vk_size,
    );
    println!();
}

#[tokio::test]
async fn compare_dust_v2_and_v3_proving() {
    // Build a real Dust spend proof preimage exactly the way current ledger
    // code does (same as `dust::tests::test_proof_size`), so both circuits
    // are measured against production-shaped witness/public inputs rather
    // than synthetic data.
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut state = TestState::<InMemoryDB>::new(&mut rng);
    state.give_fee_token(&mut rng, 1).await;
    let utxo = state.dust.utxos().next().unwrap();
    let dust_spend = state
        .dust
        .spend(&state.dust_key, &utxo, 42, state.time)
        .expect("building the dust spend witness should succeed")
        .1;
    let preimage = dust_spend.proof;

    let ir_v3: zkir_v3::IrSource =
        serialize::tagged_deserialize(&mut &V3_BZKIR[..]).expect("V3 IR should decode");
    let ir_v2 = zkir_v2::IrSource::load_from_tagged(std::io::Cursor::new(V2_BZKIR))
        .expect("V2 IR should decode");

    // Sanity check *before* timing anything: both circuits must accept the
    // very same (current-format) preimage, or the comparison below wouldn't
    // be apples-to-apples. `Zkir::check` (the current trait) is used for
    // both, since - unlike `keygen`/`prove` - it doesn't dispatch on
    // `IrMinorVersion`, so it works uniformly across the legacy V0-tagged
    // `ir_v2` and the current `ir_v3`.
    use transient_crypto::proofs::Zkir;
    ir_v3
        .check(&preimage)
        .expect("V3 circuit should accept the current dust spend preimage");
    ir_v2
        .check(&preimage)
        .expect("V2 circuit should accept the current dust spend preimage");

    let v2 = measure_v2(ir_v2, &preimage).await;
    let v3 = measure_v3(ir_v3, &preimage).await;

    print_report(&v2, &v3);

    // Basic sanity bounds, not tight assertions - the point of this test is
    // the printed comparison, not pinning exact byte counts that will drift
    // as the circuits evolve.
    assert!(v2.proof_size > 0);
    assert!(v3.proof_size > 0);
    assert!(v2.vk_size > 0);
    assert!(v3.vk_size > 0);
}
