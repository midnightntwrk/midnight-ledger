use futures::executor::block_on;
use midnight_transient_crypto::proofs::Zkir;
use midnight_serialize::{tagged_serialize, Serializable};
use midnight_transient_crypto::curve::Fr;
use midnight_transient_crypto::proofs::{
    KeyLocation, ParamsProver, ParamsProverProvider, ProofPreimage, ProvingKeyMaterial, Resolver,
    VerifierKey,
};
use midnight_zkir::IrSource;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use std::borrow::Cow;
use std::fs::{self, File};
use std::io::BufReader;

type ProverKey = midnight_transient_crypto::proofs::ProverKey<IrSource>;

struct TestResolver {
    pk: ProverKey,
    vk: VerifierKey,
    ir: IrSource,
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
        let dir = std::env::var("MIDNIGHT_PP").expect("MIDNIGHT_PP must be set");
        ParamsProver::read(BufReader::new(File::open(format!(
            "{dir}/bls_midnight_2p{k}"
        ))?))
    }
}

fn main() {
    let ir_raw = r#"{
        "version": { "major": 2, "minor": 0 },
        "num_inputs": 1,
        "do_communications_commitment": false,
        "instructions": [
            { "op": "assert", "cond": 0 }
        ]
    }"#;
    let ir = IrSource::load(ir_raw.as_bytes()).unwrap();

    let (pk, vk) = block_on(ir.keygen(&TestParams)).unwrap();

    let preimage = ProofPreimage {
        binding_input: Fr::from(42),
        communications_commitment: None,
        inputs: vec![Fr::from(1)],
        private_transcript: vec![],
        public_transcript_inputs: vec![],
        public_transcript_outputs: vec![],
        key_location: KeyLocation(Cow::Borrowed("builtin")),
    };

    let mut rng = ChaCha20Rng::from_seed([42; 32]);
    let (proof, _) = block_on(preimage.prove::<IrSource>(
        &mut rng,
        &TestParams,
        &TestResolver {
            pk,
            vk: vk.clone(),
            ir,
        },
    ))
    .unwrap();

    // The statement for this circuit: binding_input = 42
    let statement = vec![Fr::from(42)];

    // Save fixtures
    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("v1_fixtures");
    fs::create_dir_all(&out_dir).unwrap();

    // Proof: raw bytes
    fs::write(out_dir.join("proof.bin"), &proof.0).unwrap();

    // VK: serde_json
    let vk_json = serde_json::to_vec(&vk).unwrap();
    fs::write(out_dir.join("vk.json"), &vk_json).unwrap();

    // Statement: serde_json
    let statement_json = serde_json::to_vec(&statement).unwrap();
    fs::write(out_dir.join("statement.json"), &statement_json).unwrap();

    // Also save VK via Serializable for the raw-bytes path
    let mut vk_raw = Vec::new();
    Serializable::serialize(&vk, &mut vk_raw).unwrap();
    fs::write(out_dir.join("vk.bin"), &vk_raw).unwrap();

    eprintln!("Fixtures written to {}", out_dir.display());
}
