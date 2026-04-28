#![cfg(all(feature = "proof-server-http", not(target_os = "android")))]

use prover_core::{BenchOpts, ProverCore, spawn_local_server};

#[tokio::test(flavor = "multi_thread")]
async fn prove_zkir_example_via_http_matches_library() {
    let _ = tracing_subscriber::fmt::try_init();
    let cache = std::env::temp_dir().join(format!(
        "prover-core-http-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&cache);

    let pc = ProverCore::new(cache).await.expect("init");

    let ref_run = pc
        .prove_zkir_example(BenchOpts::default())
        .await
        .expect("library prove");
    assert_eq!(ref_run.verified, Some(true), "library run must verify");

    let server = spawn_local_server().await.expect("spawn server");
    let base_url = server.base_url();

    let http_run = pc
        .prove_via_http(&base_url)
        .await
        .expect("http prove");
    assert_eq!(http_run.verified, Some(true), "http run must verify");
    assert!(!http_run.proof_bytes.is_empty());

    eprintln!(
        "lib={:?}  http={:?}  ratio={:.2}x  http_bytes={}",
        ref_run.elapsed,
        http_run.elapsed,
        http_run.elapsed.as_secs_f64() / ref_run.elapsed.as_secs_f64(),
        http_run.proof_bytes.len(),
    );

    server.stop().await;
}
