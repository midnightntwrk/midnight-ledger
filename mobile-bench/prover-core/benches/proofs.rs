use criterion::{Criterion, criterion_group, criterion_main};
use prover_core::{BenchOpts, ProverCore};

fn zkir_example(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let cache = std::env::temp_dir().join("prover-core-bench");
    let _ = std::fs::remove_dir_all(&cache);
    let pc = rt.block_on(async { ProverCore::new(cache).await.expect("init") });
    c.bench_function("prove_zkir_example", |b| {
        b.iter(|| {
            rt.block_on(async {
                pc.prove_zkir_example(BenchOpts {
                    verify_after: false,
                    seed: Some(0),
                })
                .await
                .expect("prove")
            })
        })
    });
}

criterion_group!(benches, zkir_example);
criterion_main!(benches);
