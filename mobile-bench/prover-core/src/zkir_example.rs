use std::time::Instant;

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serialize::tagged_serialize;
use transient_crypto::proofs::{
    KeyLocation, PARAMS_VERIFIER, ProofPreimage, ProverKey, ProvingKeyMaterial,
    Resolver as ResolverT, VerifierKey, Zkir,
};
use zkir::IrSource;

use crate::{BenchOpts, Error, ProofRun, ProverCore, Result};

const LABEL: &str = "zkir-minimal-assert";

/// A trivial 1-input "assert(cond == 0)" circuit. Mirrors `test_minimal_proof`
/// in zkir/tests/proofs.rs — exercises the full halo2-kzg prove/verify pipeline
/// with the smallest possible circuit so we can validate it end-to-end on
/// every target (desktop, Android emulator, S24 Ultra) without depending on
/// captured contract preimages.
const MINIMAL_IR_JSON: &str = r#"{
    "version": { "major": 2, "minor": 0 },
    "num_inputs": 1,
    "do_communications_commitment": false,
    "instructions": [
        { "op": "assert", "cond": 0 }
    ]
}"#;

/// Holds keygen output so we only setup once per ProverCore lifetime.
struct ZkirExampleResolver {
    pk: ProverKey<IrSource>,
    vk: VerifierKey,
    ir: IrSource,
}

impl ResolverT for ZkirExampleResolver {
    async fn resolve_key(
        &self,
        _key: KeyLocation,
    ) -> std::io::Result<Option<ProvingKeyMaterial>> {
        let mut prover_key = Vec::new();
        tagged_serialize(&self.pk, &mut prover_key)?;
        let mut verifier_key = Vec::new();
        tagged_serialize(&self.vk, &mut verifier_key)?;
        let mut ir_source = Vec::new();
        tagged_serialize(&self.ir, &mut ir_source)?;
        Ok(Some(ProvingKeyMaterial {
            prover_key,
            verifier_key,
            ir_source,
        }))
    }
}

impl ProverCore {
    pub async fn prove_zkir_example(&self, opts: BenchOpts) -> Result<ProofRun> {
        let seed = opts.seed.unwrap_or(0x42);
        let mut rng = ChaCha20Rng::seed_from_u64(seed);

        let ir = IrSource::load(MINIMAL_IR_JSON.as_bytes())
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("load ir: {e}")))?;

        let (pk, vk) = ir
            .keygen(&self.params.zswap.0)
            .await
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("keygen: {e}")))?;
        let k = ir.k();

        let resolver = ZkirExampleResolver {
            pk,
            vk: vk.clone(),
            ir: ir.clone(),
        };

        // binding_input becomes the sole public input the verifier checks.
        let binding_input = transient_crypto::curve::Fr::from(42u64);
        let preimage = ProofPreimage {
            inputs: vec![transient_crypto::curve::Fr::from(1u64)],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            binding_input,
            communications_commitment: None,
            key_location: KeyLocation(std::borrow::Cow::Borrowed("minimal")),
        };

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
            // PARAMS_VERIFIER is embedded for k <= 14; binding_input is the
            // single public input.
            let ok = vk
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
