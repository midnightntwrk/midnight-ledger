//! Criterion benchmarks for crypto operations.
//!
//! Used to learn cost models in :/generate-cost-model.
//!
//! See :/onchain-runtime/benchmarks/benchmarking.rs for notes on how to run
//! criterion benchmarks. E.g. run only the `transient_hash` benchmark, in
//! "quick" mode, with
//!
//! ```text
//! cargo bench -p midnight-transient-crypto --bench benchmarking -- '/^transient_hash/$' --quick
//! ```
//!
//! Running all crypto benches takes ~2.5 minutes in `--quick` mode and ~25
//! minutes in normal mode on @ntc2's machine.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use midnight_circuits::compact_std_lib::Relation;
use midnight_circuits::instructions::{AssignmentInstructions, PublicInputInstructions};
use midnight_circuits::types::AssignedNative;
use midnight_transient_crypto::commitment::PureGeneratorPedersen;
use midnight_transient_crypto::curve::{EmbeddedFr, EmbeddedGroupAffine, Fr, outer};
use midnight_transient_crypto::hash::{hash_to_curve, transient_hash};
use rand::RngCore;
use rand::{Rng, rngs::OsRng};
use serde_json::json;
use serialize::{Serializable, tagged_serialize};

/// Helper function to run benchmarks with multiple data points for zero-parameter operations.
fn with_json_iter<F>(group_name: &str, c: &mut Criterion, mut benchmark_fn: F)
where
    F: FnMut(&mut criterion::BenchmarkGroup<criterion::measurement::WallTime>, serde_json::Value),
{
    let mut group = c.benchmark_group(group_name);
    for i in 0..10 {
        let json = json!({
            "container_type": "none",
            "uid": i
        });
        benchmark_fn(&mut group, json);
    }
    group.finish();
}

pub fn hash(c: &mut Criterion) {
    rayon::ThreadPoolBuilder::new()
        .use_current_thread()
        .num_threads(1)
        .build_global()
        .unwrap();
    with_json_iter("transient_hash", c, |g, json| {
        g.bench_function(json.to_string(), |b| {
            b.iter(|| {
                black_box(transient_hash(black_box(&[
                    Fr::from(OsRng.r#gen::<u64>()),
                    Fr::from(OsRng.r#gen::<u64>()),
                ])))
            });
        });
    });
    with_json_iter("hash_to_curve", c, |g, json| {
        g.bench_function(json.to_string(), |b| {
            b.iter(|| {
                black_box(hash_to_curve(black_box(&[
                    Fr::from(OsRng.r#gen::<u64>()),
                    Fr::from(OsRng.r#gen::<u64>()),
                ])))
            });
        });
    });
}

pub fn embedded_point_arith(c: &mut Criterion) {
    with_json_iter("ec_add", c, |g, json| {
        g.bench_function(json.to_string(), |bench| {
            let a: EmbeddedGroupAffine = OsRng.r#gen();
            let b: EmbeddedGroupAffine = OsRng.r#gen();
            bench.iter(|| black_box(a + b));
        });
    });
    with_json_iter("ec_mul", c, |g, json| {
        g.bench_function(json.to_string(), |bench| {
            let a: EmbeddedFr = OsRng.r#gen();
            let b: EmbeddedGroupAffine = OsRng.r#gen();
            bench.iter(|| black_box(b * a));
        });
    });
}

pub fn proof_verification(c: &mut Criterion) {
    use futures::executor::block_on;
    use midnight_transient_crypto::proofs::*;
    use serialize::{Deserializable, Serializable, Tagged};
    use std::borrow::Cow;
    use std::fs::File;
    use std::io::BufReader;

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
    let json = json!({
                "container_type": "none",
    });
    let mut group = c.benchmark_group("proof_verify");
    let mut test_vk = None;
    for size in [
        1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 6000, 8192, 10000, 12000, 14000,
    ] {
        let ir = TestIr { no_inputs: size };

        let (pk, vk) = block_on(ir.keygen(&TestParams)).unwrap();
        if test_vk.is_none() {
            test_vk = Some(vk.clone());
        }
        let inp = (0..size).map(|_| OsRng.r#gen()).collect::<Vec<_>>();
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
            &mut OsRng,
            &TestParams,
            &TestResolver {
                pk: pk.clone(),
                vk: vk.clone(),
                ir: ir.clone(),
            },
        ))
        .unwrap();
        let mut json = json.clone();
        json["size"] = size.into();
        group.bench_function(json.to_string(), |b| {
            b.iter(|| {
                black_box(&vk)
                    .verify(
                        &PARAMS_VERIFIER,
                        black_box(&proof),
                        black_box(inp.iter().copied()),
                    )
                    .unwrap()
            });
        });
    }
    group.finish();
    let mut ser = Vec::new();
    Serializable::serialize(&test_vk.unwrap(), &mut ser).unwrap();
    with_json_iter("verifier_key_load", c, |g, json| {
        g.bench_function(json.to_string(), |b| {
            b.iter(|| {
                let des: VerifierKey = Deserializable::deserialize(&mut &ser[..], 0).unwrap();
                des.init().unwrap();
                des
            });
        });
    });
}

pub fn fr_arith(c: &mut Criterion) {
    with_json_iter("fr_add", c, |g, json| {
        g.bench_function(json.to_string(), |bench| {
            let a: Fr = OsRng.r#gen();
            let b: Fr = OsRng.r#gen();
            bench.iter(|| black_box(a + b));
        });
    });

    with_json_iter("fr_mul", c, |g, json| {
        g.bench_function(json.to_string(), |bench| {
            let a: Fr = OsRng.r#gen();
            let b: Fr = OsRng.r#gen();
            bench.iter(|| black_box(a * b));
        });
    });
}

pub fn pedersen(c: &mut Criterion) {
    with_json_iter("pedersen_valid", c, |g, json| {
        // Random value in 0..=1000
        let len = OsRng.r#gen::<u64>() % 1000;
        let challenge: Vec<u8> = (0..len).map(|_| rand::random()).collect();
        let pedersen = PureGeneratorPedersen::new_from(&mut OsRng, &OsRng.r#gen(), &challenge);
        g.bench_function(json.to_string(), |b| {
            b.iter(|| black_box(&pedersen).valid(black_box(&challenge)));
        });
    });
}

pub fn signature_verification(c: &mut Criterion) {
    let mut group = c.benchmark_group("signature_verify");
    for size in [
        10, 100, 1000, 5000, 10000, 30000, 50000, 70000, 90000, 100000,
    ] {
        let json = json!({
            "container_type": "none",
            "size": size,
        });
        group.bench_function(json.to_string(), |b| {
            let mut msg = vec![0u8; size];
            OsRng.fill_bytes(&mut msg);
            let sk = base_crypto::signatures::SigningKey::sample(OsRng);
            let vk = sk.verifying_key();
            let sig = sk.sign(&mut OsRng, &msg);
            b.iter(|| vk.verify(black_box(&msg), black_box(&sig)))
        });
    }
    group.finish();
}

criterion_group!(
    benchmarking,
    hash,
    embedded_point_arith,
    fr_arith,
    proof_verification,
    pedersen,
    signature_verification
);
criterion_main!(benchmarking);
