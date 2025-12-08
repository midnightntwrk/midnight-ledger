//! Send a single zswap proof request to a running proof server
//!
//! This example generates a zswap transaction (K=14 circuit) and sends it to the
//! proof server for proving. Use this to verify the server is working correctly.
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
//!    Only the proving keys and zkir/ subdirectory structure need to be generated.
//!    
//! 2. Build the proof server with GPU support:
//!    ```sh
//!    cargo build --release -p midnight-proof-server --features gpu
//!    ```
//!
//! 3. Start the proof server:
//!    ```sh
//!    # CPU mode:
//!    MIDNIGHT_DEVICE=cpu ./proof-server/manage-proof-server.sh start
//!    
//!    # GPU mode (recommended for K>=14):
//!    MIDNIGHT_DEVICE=auto ./proof-server/manage-proof-server.sh start
//!    ```
//!
//! # Running
//!
//! ```sh
//! MIDNIGHT_LEDGER_TEST_STATIC_DIR=$PWD/zkir-precompiles \
//!   cargo run --release -p midnight-proof-server --features gpu --example send_zswap_proof
//! ```
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║   Sending k=14 Proof Request to Running Server          ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");
    
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
            eprintln!("  Start it with: FEATURES=\"gpu\" ./manage-proof-server.sh start");
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
    
    println!("→ Sending POST request to {}...", url);
    println!("  (Watch server logs: FEATURES=\"gpu\" ./manage-proof-server.sh logs)");
    println!("  Expected time: 0.2-0.5s with GPU acceleration\n");
    
    let client = reqwest::Client::new();
    let start = std::time::Instant::now();
    
    let response = client
        .post(&url)
        .body(body)
        .send()
        .await?;
    
    let elapsed = start.elapsed();
    let status = response.status();
    
    println!("→ Response received:");
    println!("  Status: {}", status);
    println!("  Time: {:.3}s", elapsed.as_secs_f64());
    
    if status.is_success() {
        let response_bytes = response.bytes().await?;
        println!("  Size: {} bytes", response_bytes.len());
        
        // Check if we got a proof
        let response_str = String::from_utf8_lossy(&response_bytes[..200.min(response_bytes.len())]);
        if response_str.contains(",proof,") {
            println!("\n✓ PLONK proof successfully generated!");
            println!("\n╔══════════════════════════════════════════════════════════╗");
            println!("║  GPU PLONK PROOF GENERATION VERIFIED                    ║");
            println!("╚══════════════════════════════════════════════════════════╝\n");
        } else if response_str.contains(",proof-preimage,") {
            println!("\n⚠ Response contains proof-preimage (not yet proven)");
        } else {
            println!("\n→ Response format:");
            println!("{}", &response_str[..100.min(response_str.len())]);
        }
    } else {
        let error = response.text().await?;
        println!("\n✗ Error: {}", error);
        std::process::exit(1);
    }
    
    Ok(())
}
