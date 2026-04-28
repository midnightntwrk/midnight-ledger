use prover_core::{BenchOpts, ProverCore};

#[tokio::test]
async fn prove_zkir_example_succeeds_and_verifies() {
    let _ = tracing_subscriber::fmt::try_init();
    let cache = std::env::temp_dir().join(format!(
        "prover-core-zkir-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&cache);

    let pc = ProverCore::new(cache).await.expect("init");
    let run = pc
        .prove_zkir_example(BenchOpts::default())
        .await
        .expect("prove_zkir_example");

    assert!(!run.proof_bytes.is_empty(), "proof should have bytes");
    assert_eq!(run.verified, Some(true), "verify must succeed");
    assert!(run.elapsed.as_millis() > 0);
    eprintln!(
        "zkir-example: prove={:?} verify={:?} bytes={}",
        run.elapsed,
        run.verify_elapsed,
        run.proof_bytes.len()
    );
}
