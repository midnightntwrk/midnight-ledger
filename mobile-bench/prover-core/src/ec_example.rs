use std::time::Instant;

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serialize::tagged_serialize;
use transient_crypto::curve::{EmbeddedGroupAffine, Fr};
use transient_crypto::proofs::{PARAMS_VERIFIER, ProofPreimage, Zkir};
use zkir::IrSource;

use crate::resolver::{ExampleResolver, make_preimage};
use crate::{BenchOpts, Error, ProofRun, ProverCore, Result};

pub(crate) const LABEL: &str = "zkir-ec-mul-add";

/// 4-input curve circuit: ec_mul + ec_mul_generator + ec_add. Mirrors
/// `test_ec_proof` in zkir/tests/proofs.rs:352. Witness layout produced by
/// the IR is `[a_x, a_y, scalar, scalar_g, mul_x, mul_y, gen_x, gen_y]` —
/// indices 4..8 are the outputs of the first two ops, which `ec_add` then
/// consumes.
const EC_IR_JSON: &str = r#"{
    "version": { "major": 2, "minor": 0 },
    "num_inputs": 4,
    "do_communications_commitment": false,
    "instructions": [
        { "op": "ec_mul", "a_x": 0, "a_y": 1, "scalar": 2 },
        { "op": "ec_mul_generator", "scalar": 3 },
        { "op": "ec_add", "a_x": 4, "a_y": 5, "b_x": 6, "b_y": 7 }
    ]
}"#;

const BINDING_INPUT_RAW: u64 = 42;

fn ec_preimage() -> ProofPreimage {
    let g = EmbeddedGroupAffine::generator();
    let inputs = vec![
        g.x().expect("generator has x"),
        g.y().expect("generator has y"),
        Fr::from(42u64),
        Fr::from(63u64),
    ];
    make_preimage(inputs, Fr::from(BINDING_INPUT_RAW), "builtin")
}

impl ProverCore {
    async fn keygen_ec(&self) -> Result<ExampleResolver> {
        let ir = IrSource::load(EC_IR_JSON.as_bytes())
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("load ir: {e}")))?;
        let (pk, vk) = ir
            .keygen(&self.params.zswap.0)
            .await
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("keygen: {e}")))?;
        Ok(ExampleResolver { pk, vk, ir })
    }

    pub async fn prove_ec_example(&self, opts: BenchOpts) -> Result<ProofRun> {
        let seed = opts.seed.unwrap_or(0x42);
        let mut rng = ChaCha20Rng::seed_from_u64(seed);

        let resolver = self.keygen_ec().await?;
        let k = resolver.ir.k();
        let preimage = ec_preimage();
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
