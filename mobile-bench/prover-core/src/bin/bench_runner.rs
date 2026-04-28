//! Standalone bench runner: runs `prove_zkir_example` once and prints a
//! single JSON line. Cross-compiles to aarch64-linux-android with cargo-ndk
//! so we can adb-push it onto an emulator or device and get real latency
//! numbers without going through Dioxus / cargo-apk.

use std::path::PathBuf;
use std::process::ExitCode;

use prover_core::{BenchOpts, ProverCore};

#[derive(serde::Serialize)]
struct Output {
    target: &'static str,
    label: &'static str,
    k: u8,
    prove_ms: u128,
    verify_ms: Option<u128>,
    verified: Option<bool>,
    proof_bytes: usize,
}

fn target_string() -> &'static str {
    if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}

fn main() -> ExitCode {
    let cache = std::env::var_os("BENCH_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("midnight-bench-runner"));
    let _ = std::fs::remove_dir_all(&cache);

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("tokio runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    let res = rt.block_on(async move {
        let pc = ProverCore::new(cache).await?;
        pc.prove_zkir_example(BenchOpts::default()).await
    });

    match res {
        Ok(run) => {
            let out = Output {
                target: target_string(),
                label: run.label,
                k: run.k,
                prove_ms: run.elapsed.as_millis(),
                verify_ms: run.verify_elapsed.map(|d| d.as_millis()),
                verified: run.verified,
                proof_bytes: run.proof_bytes.len(),
            };
            println!("{}", serde_json::to_string(&out).unwrap());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("prove failed: {e}");
            ExitCode::FAILURE
        }
    }
}
