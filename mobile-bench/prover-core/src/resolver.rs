use serialize::tagged_serialize;
use transient_crypto::proofs::{
    KeyLocation, ProofPreimage, ProverKey, ProvingKeyMaterial, Resolver as ResolverT,
    VerifierKey,
};
use zkir::IrSource;

/// Holds keygen output for a single zkir example circuit.
///
/// All in-process example proofs (zkir / htc / ec) use the same resolver
/// shape — the IR JSON, inputs, and label change but the prove/verify wiring
/// is identical. Sharing this struct keeps each example module to just the
/// circuit-specific bits.
pub(crate) struct ExampleResolver {
    pub(crate) pk: ProverKey<IrSource>,
    pub(crate) vk: VerifierKey,
    pub(crate) ir: IrSource,
}

impl ResolverT for ExampleResolver {
    async fn resolve_key(
        &self,
        _key: KeyLocation,
    ) -> std::io::Result<Option<ProvingKeyMaterial>> {
        Ok(Some(self.proving_key_material()?))
    }
}

impl ExampleResolver {
    pub(crate) fn proving_key_material(&self) -> std::io::Result<ProvingKeyMaterial> {
        let mut prover_key = Vec::new();
        tagged_serialize(&self.pk, &mut prover_key)?;
        let mut verifier_key = Vec::new();
        tagged_serialize(&self.vk, &mut verifier_key)?;
        let mut ir_source = Vec::new();
        tagged_serialize(&self.ir, &mut ir_source)?;
        Ok(ProvingKeyMaterial {
            prover_key,
            verifier_key,
            ir_source,
        })
    }
}

/// Convenience: build a `ProofPreimage` with no transcripts and a fixed
/// `binding_input`. All three iter-1/iter-2 example circuits share this shape.
pub(crate) fn make_preimage(
    inputs: Vec<transient_crypto::curve::Fr>,
    binding_input: transient_crypto::curve::Fr,
    key_label: &'static str,
) -> ProofPreimage {
    ProofPreimage {
        inputs,
        private_transcript: vec![],
        public_transcript_inputs: vec![],
        public_transcript_outputs: vec![],
        binding_input,
        communications_commitment: None,
        key_location: KeyLocation(std::borrow::Cow::Borrowed(key_label)),
    }
}
