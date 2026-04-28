use prover_core::ProverCore;
use std::path::PathBuf;

#[tokio::test]
async fn params_cache_initialises_in_isolated_dir() {
    let dir = tempdir_for_test("params_cache_initialises");
    let pc = ProverCore::new(dir.clone()).await.expect("init");

    assert!(pc.cache_dir().exists());
    assert_eq!(pc.cache_dir(), dir);
}

fn tempdir_for_test(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("prover-core-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    p
}
