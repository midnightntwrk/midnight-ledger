use std::path::PathBuf;

use prover_core::{BenchOpts, ProofRun, ProverCore};

fn isolated_cache(tag: &str) -> PathBuf {
    let cache = std::env::temp_dir().join(format!(
        "prover-core-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&cache);
    cache
}

fn assert_proof_run(run: &ProofRun, expected_label: &str) {
    assert_eq!(run.label, expected_label);
    assert!(!run.proof_bytes.is_empty(), "proof should have bytes");
    assert_eq!(run.verified, Some(true), "verify must succeed");
    assert!(run.elapsed.as_millis() > 0);
    eprintln!(
        "{}: k={} prove={:?} verify={:?} bytes={}",
        run.label,
        run.k,
        run.elapsed,
        run.verify_elapsed,
        run.proof_bytes.len()
    );
}

#[tokio::test]
async fn prove_zkir_example_succeeds_and_verifies() {
    let _ = tracing_subscriber::fmt::try_init();
    let pc = ProverCore::new(isolated_cache("zkir")).await.expect("init");
    let run = pc
        .prove_zkir_example(BenchOpts::default())
        .await
        .expect("prove_zkir_example");
    assert_proof_run(&run, "zkir-minimal-assert");
    assert_eq!(run.k, 4);
}

#[tokio::test]
async fn prove_htc_example_succeeds_and_verifies() {
    let _ = tracing_subscriber::fmt::try_init();
    let pc = ProverCore::new(isolated_cache("htc")).await.expect("init");
    let run = pc
        .prove_htc_example(BenchOpts::default())
        .await
        .expect("prove_htc_example");
    assert_proof_run(&run, "zkir-hash-to-curve");
    assert_eq!(run.k, 9);
}

#[tokio::test]
async fn prove_ec_example_succeeds_and_verifies() {
    let _ = tracing_subscriber::fmt::try_init();
    let pc = ProverCore::new(isolated_cache("ec")).await.expect("init");
    let run = pc
        .prove_ec_example(BenchOpts::default())
        .await
        .expect("prove_ec_example");
    assert_proof_run(&run, "zkir-ec-mul-add");
    assert_eq!(run.k, 11);
}
