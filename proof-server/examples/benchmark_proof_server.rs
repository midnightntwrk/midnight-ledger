//! Benchmark proof server end-to-end performance
//!
//! This sends multiple proof requests to the running proof server and measures timing.
//! Use this to compare CPU vs GPU proof generation performance.
//!
//! # Prerequisites
//!
//! 1. Generate prover/verifier keys (if not using Nix):
//!    ```sh
//!    # Build zkir tool
//!    cargo build --release -p zkir --features binary
//!    
//!    # Setup zkir directory structure (required by test_resolver)
//!    cd zkir-precompiles/zswap
//!    mkdir -p zkir keys
//!    cp *.bzkir zkir/
//!    cd ../..
//!    
//!    # Generate proving keys from pre-compiled .zkir files
//!    ./target/release/zkir compile-many zkir-precompiles/zswap zkir-precompiles/zswap/keys
//!    ```
//!    This generates output.prover/verifier, sign.prover/verifier, and spend.prover/verifier
//!    in zkir-precompiles/zswap/keys/ (takes ~30 seconds).
//!    
//!    Note: The .bzkir files are already pre-compiled and committed to the repository.
//!    Only the proving keys and zkir/ subdirectory structure need to be generated
//!    (shared with send_zswap_proof example).
//!    Note: Nix users can skip this - keys are auto-generated in the dev shell.
//!    
//! 2. Build the proof server with GPU support:
//!    ```sh
//!    cargo build --release -p midnight-proof-server --features gpu
//!    ```
//!
//! 3. Start the proof server:
//!    ```sh
//!    # For CPU baseline:
//!    MIDNIGHT_DEVICE=cpu ./proof-server/manage-proof-server.sh start
//!    
//!    # For GPU acceleration (K>=14):
//!    MIDNIGHT_DEVICE=auto ./proof-server/manage-proof-server.sh start
//!    ```
//!
//! # Running
//!
//! ```sh
//! MIDNIGHT_LEDGER_TEST_STATIC_DIR=$PWD/zkir-precompiles \
//!   cargo run --release -p midnight-proof-server --features gpu --example benchmark_proof_server
//! ```
//!
//! # Comparing CPU vs GPU
//!
//! 1. Start server with `MIDNIGHT_DEVICE=cpu`, run benchmark, note times
//! 2. Restart server with `MIDNIGHT_DEVICE=auto`, run benchmark again
//! 3. Compare the \"Avg (warm)\" times to see GPU speedup
//!
//! # Environment Variables
//!
//! - `MIDNIGHT_LEDGER_TEST_STATIC_DIR`: Path to zkir-precompiles directory (required)
//! - `MIDNIGHT_PROOF_SERVER_PORT`: Server port (default: 6300)

use ledger::test_utilities::{serialize_request_body, test_resolver};
use ledger::structure::{Transaction, ProofPreimageMarker};
use base_crypto::signatures::Signature;
use coin_structure::coin;
use storage::db::InMemoryDB;
use transient_crypto::commitment::PedersenRandomness;
use zswap::Delta;
use rand::{rngs::StdRng, SeedableRng};
use std::time::{Duration, Instant};

const NUM_ITERATIONS: usize = 5;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n╔══════════════════════════════════════════════════════════════════════════╗");
    println!("║   Proof Server End-to-End Benchmark (K=14 zswap proofs)                  ║");
    println!("╚══════════════════════════════════════════════════════════════════════════╝\n");
    
    let port = std::env::var("MIDNIGHT_PROOF_SERVER_PORT")
        .unwrap_or_else(|_| "6300".to_string());
    let url = format!("http://localhost:{}/prove-tx", port);
    
    println!("→ Target server: {}", url);
    
    // Check if server is running
    let health_url = format!("http://localhost:{}/health", port);
    match reqwest::get(&health_url).await {
        Ok(resp) if resp.status().is_success() => {
            println!("✓ Server is running\n");
        }
        _ => {
            eprintln!("✗ Server not responding on port {}", port);
            eprintln!("  Start it with: MIDNIGHT_DEVICE=cpu ./manage-proof-server.sh start");
            eprintln!("  Or:            MIDNIGHT_DEVICE=gpu ./manage-proof-server.sh start");
            std::process::exit(1);
        }
    }
    
    println!("→ Generating zswap transaction (k=14, 16,134 rows)...");
    
    let resolver = test_resolver("zswap");
    let mut rng = StdRng::seed_from_u64(0x42);
    
    // Create zswap transaction with outputs
    let mut outputs = storage::storage::Array::new();
    let claim_amount = 100;
    
    let coin = coin::Info::new(&mut rng, claim_amount, Default::default());
    let sks = zswap::keys::SecretKeys::from_rng_seed(&mut rng);
    let out = zswap::Output::new::<_>(
        &mut rng,
        &coin,
        0,
        &sks.coin_public_key(),
        Some(sks.enc_public_key()),
    )?;
    outputs = outputs.push(out);
    
    let deltas = [Delta {
        token_type: Default::default(),
        value: -(claim_amount as i128),
    }]
    .into_iter()
    .collect();

    let mut offer = zswap::Offer {
        inputs: vec![].into(),
        outputs,
        transient: vec![].into(),
        deltas,
    };
    offer.normalize();
    
    let tx: Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB> =
        Transaction::new("k14-gpu-test", Default::default(), Some(offer), Default::default());
    
    println!("✓ Transaction generated\n");
    
    println!("→ Serializing transaction with proof preimages...");
    #[allow(deprecated)]
    let body = serialize_request_body(&tx, &resolver).await
        .map_err(|e| format!("Serialization error: {:?}", e))?;
    println!("✓ Request body: {} bytes\n", body.len());
    
    println!("┌───────────┬────────────┬────────────┐");
    println!("│ Iteration │   Status   │    Time    │");
    println!("├───────────┼────────────┼────────────┤");
    
    let client = reqwest::Client::new();
    let mut times: Vec<Duration> = Vec::new();
    
    for i in 1..=NUM_ITERATIONS {
        let start = Instant::now();
        
        let response = client
            .post(&url)
            .body(body.clone())
            .send()
            .await?;
        
        let elapsed = start.elapsed();
        let status = response.status();
        
        if status.is_success() {
            times.push(elapsed);
            println!(
                "│     {:2}    │   ✓ OK     │ {:>8}   │",
                i,
                format_duration(elapsed)
            );
        } else {
            println!(
                "│     {:2}    │   ✗ {:3}   │ {:>8}   │",
                i,
                status.as_u16(),
                format_duration(elapsed)
            );
        }
    }
    
    println!("└───────────┴────────────┴────────────┘\n");
    
    if !times.is_empty() {
        let total: Duration = times.iter().sum();
        let avg = total / times.len() as u32;
        let min = times.iter().min().unwrap();
        let max = times.iter().max().unwrap();
        
        // Skip first run for "warm" average
        let warm_times: Vec<_> = times.iter().skip(1).collect();
        let warm_avg = if !warm_times.is_empty() {
            let warm_total: Duration = warm_times.iter().copied().copied().sum();
            warm_total / warm_times.len() as u32
        } else {
            avg
        };
        
        println!("╔══════════════════════════════════════╗");
        println!("║           Performance Summary        ║");
        println!("╠══════════════════════════════════════╣");
        println!("║  First run:  {:>10}             ║", format_duration(times[0]));
        println!("║  Min:        {:>10}             ║", format_duration(*min));
        println!("║  Max:        {:>10}             ║", format_duration(*max));
        println!("║  Avg (all):  {:>10}             ║", format_duration(avg));
        println!("║  Avg (warm): {:>10}             ║", format_duration(warm_avg));
        println!("╚══════════════════════════════════════╝\n");
        
        println!("Notes:");
        println!("  • K=14 circuit with 16,134 rows");
        println!("  • First run may include GPU warmup overhead");
        println!("  • 'Warm' average excludes first run");
        println!("  • Compare CPU vs GPU by restarting server with different MIDNIGHT_DEVICE");
    }
    
    Ok(())
}

fn format_duration(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms >= 1000.0 {
        format!("{:.3}s", ms / 1000.0)
    } else {
        format!("{:.1}ms", ms)
    }
}
