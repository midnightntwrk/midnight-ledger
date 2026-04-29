use std::time::Instant;

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serialize::tagged_serialize;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{PARAMS_VERIFIER, ProofPreimage, Zkir};
use zkir::IrSource;

use crate::resolver::{ExampleResolver, make_preimage};
use crate::{BenchOpts, Error, ProofRun, ProverCore, Result};

pub(crate) const LABEL: &str = "zkir-hash-to-curve";

/// 3-input hash-to-curve circuit. Mirrors `test_htc_proof` in
/// zkir/tests/proofs.rs:162 — exercises the in-circuit hash + curve
/// mapping primitive used by Pedersen commits and signature checks.
const HTC_IR_JSON: &str = r#"{
    "version": { "major": 2, "minor": 0 },
    "num_inputs": 3,
    "do_communications_commitment": false,
    "instructions": [
        { "op": "hash_to_curve", "inputs": [0, 1, 2] }
    ]
}"#;

const BINDING_INPUT_RAW: u64 = 42;

fn htc_preimage() -> ProofPreimage {
    make_preimage(
        vec![Fr::from(1u64), Fr::from(2u64), Fr::from(3u64)],
        Fr::from(BINDING_INPUT_RAW),
        "builtin",
    )
}

impl ProverCore {
    async fn keygen_htc(&self) -> Result<ExampleResolver> {
        let ir = IrSource::load(HTC_IR_JSON.as_bytes())
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("load ir: {e}")))?;
        let (pk, vk) = ir
            .keygen(&self.params.zswap.0)
            .await
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("keygen: {e}")))?;
        Ok(ExampleResolver { pk, vk, ir })
    }

    pub async fn prove_htc_example(&self, opts: BenchOpts) -> Result<ProofRun> {
        let seed = opts.seed.unwrap_or(0x42);
        let mut rng = ChaCha20Rng::seed_from_u64(seed);

        let resolver = self.keygen_htc().await?;
        let k = resolver.ir.k();
        let preimage = htc_preimage();
        let binding_input = preimage.binding_input;

        let started = Instant::now();
        let (proof, _pi_skips) = preimage
            .prove::<IrSource>(&mut rng, &self.params.zswap.0, &resolver)
            .await
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("prove: {e}")))?;
        let elapsed = started.elapsed();

        let proof_bytes = {
            let mut buf = Vec::new();
            tagged_serialize(&proof, &mut buf)
                .map_err(|e| Error::Anyhow(anyhow::anyhow!("serialize proof: {e}")))?;
            buf
        };

        let (verified, verify_elapsed) = if opts.verify_after {
            let v_started = Instant::now();
            let ok = resolver
                .vk
                .verify(&PARAMS_VERIFIER, &proof, std::iter::once(binding_input))
                .is_ok();
            (Some(ok), Some(v_started.elapsed()))
        } else {
            (None, None)
        };

        Ok(ProofRun {
            label: LABEL,
            k,
            elapsed,
            verify_elapsed,
            verified,
            proof_bytes,
        })
    }
}
