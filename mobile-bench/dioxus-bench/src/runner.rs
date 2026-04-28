use std::sync::Arc;
use std::time::Duration;

use prover_core::{BenchOpts, ProofRun, ProverCore};

#[derive(Debug, Clone)]
pub enum RunStatus {
    Idle,
    Proving(&'static str),
    Done,
    Error(String),
}

#[derive(Clone)]
pub struct Runner {
    inner: Arc<ProverCore>,
}

impl Runner {
    pub async fn new() -> Result<Self, String> {
        let dir = crate::platform::cache_dir();
        ProverCore::new(dir)
            .await
            .map(|pc| Self { inner: Arc::new(pc) })
            .map_err(|e| e.to_string())
    }

    pub async fn run_zkir(&self) -> Result<ProofRun, String> {
        self.inner
            .prove_zkir_example(BenchOpts::default())
            .await
            .map_err(|e| e.to_string())
    }
}

pub fn fmt_duration(d: Duration) -> String {
    if d.as_secs() >= 1 {
        format!("{:.2} s", d.as_secs_f64())
    } else {
        format!("{} ms", d.as_millis())
    }
}
