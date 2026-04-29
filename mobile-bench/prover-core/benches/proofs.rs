use criterion::{Criterion, criterion_group, criterion_main};
use prover_core::{BenchOpts, ProverCore};

/// Builds one fresh `ProverCore` per top-level group call. Keygen is run on
/// every iteration because that's the latency the mobile benchmark cares
/// about — we're measuring the wallet-side end-to-end "press button → get
/// proof" path, not steady-state proving.
fn run_bench<F, Fut>(c: &mut Criterion, name: &'static str, f: F)
where
    F: Fn(&'static ProverCore) -> Fut + 'static,
    Fut: std::future::Future<Output = ()>,
{
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let cache = std::env::temp_dir().join(format!("prover-core-bench-{name}"));
    let _ = std::fs::remove_dir_all(&cache);
    // Leak the ProverCore so the closure can be 'static — benches only run
    // for the lifetime of the process.
    let pc: &'static ProverCore = Box::leak(Box::new(
        rt.block_on(async { ProverCore::new(cache).await.expect("init") }),
    ));
    c.bench_function(name, |b| {
        b.iter(|| rt.block_on(f(pc)));
    });
}

fn zkir_example(c: &mut Criterion) {
    run_bench(c, "prove_zkir_example", |pc| async move {
        pc.prove_zkir_example(BenchOpts {
            verify_after: false,
            seed: Some(0),
        })
        .await
        .expect("prove");
    });
}

fn htc_example(c: &mut Criterion) {
    run_bench(c, "prove_htc_example", |pc| async move {
        pc.prove_htc_example(BenchOpts {
            verify_after: false,
            seed: Some(0),
        })
        .await
        .expect("prove");
    });
}

fn ec_example(c: &mut Criterion) {
    run_bench(c, "prove_ec_example", |pc| async move {
        pc.prove_ec_example(BenchOpts {
            verify_after: false,
            seed: Some(0),
        })
        .await
        .expect("prove");
    });
}

criterion_group!(benches, zkir_example, htc_example, ec_example);
criterion_main!(benches);
