//! Standalone bench runner: runs one of the example proofs and prints a
//! single JSON line per surface. Cross-compiles to aarch64-linux-android with
//! cargo-ndk so we can adb-push it onto an emulator or device and get real
//! latency numbers without going through Dioxus / cargo-apk.
//!
//! Usage:
//!   bench-runner                      # default: zkir
//!   bench-runner zkir                 # minimal assert circuit
//!   bench-runner htc                  # hash-to-curve circuit
//!   bench-runner ec                   # ec_mul + ec_add circuit
//!   bench-runner all                  # runs all surfaces in order, one JSON line each

use std::path::PathBuf;
use std::process::ExitCode;

use prover_core::{BenchOpts, ProofRun, ProverCore};

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

#[derive(Clone, Copy)]
enum Surface {
    Zkir,
    Htc,
    Ec,
}

impl Surface {
    fn parse(s: &str) -> Option<Vec<Surface>> {
        match s {
            "zkir" => Some(vec![Surface::Zkir]),
            "htc" => Some(vec![Surface::Htc]),
            "ec" => Some(vec![Surface::Ec]),
            "all" => Some(vec![Surface::Zkir, Surface::Htc, Surface::Ec]),
            _ => None,
        }
    }
}

async fn run_one(pc: &ProverCore, surface: Surface) -> prover_core::Result<ProofRun> {
    let opts = BenchOpts::default();
    match surface {
        Surface::Zkir => pc.prove_zkir_example(opts).await,
        Surface::Htc => pc.prove_htc_example(opts).await,
        Surface::Ec => pc.prove_ec_example(opts).await,
    }
}

fn main() -> ExitCode {
    let arg = std::env::args().nth(1).unwrap_or_else(|| "zkir".into());
    let Some(surfaces) = Surface::parse(&arg) else {
        eprintln!("unknown surface: {arg}; expected one of zkir|htc|ec|all");
        return ExitCode::FAILURE;
    };

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
        let mut runs = Vec::with_capacity(surfaces.len());
        for s in surfaces {
            runs.push(run_one(&pc, s).await?);
        }
        Ok::<_, prover_core::Error>(runs)
    });

    match res {
        Ok(runs) => {
            for run in runs {
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
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("prove failed: {e}");
            ExitCode::FAILURE
        }
    }
}
