//! prover-core: a thin embeddable wrapper around Midnight's proving primitives
//! used by both the dioxus-bench app and `cargo test`/`cargo bench`.
//!
//! See `docs/superpowers/specs/2026-04-28-mobile-proof-bench-design.md`.

#![deny(unreachable_pub)]
#![deny(warnings)]

use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("anyhow: {0}")]
    Anyhow(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct BenchOpts {
    pub verify_after: bool,
    pub seed: Option<u64>,
}

impl Default for BenchOpts {
    fn default() -> Self {
        Self { verify_after: true, seed: Some(0x42) }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProofRun {
    pub label: &'static str,
    pub k: u8,
    pub elapsed: Duration,
    pub verify_elapsed: Option<Duration>,
    pub verified: Option<bool>,
    pub proof_bytes: Vec<u8>,
}

pub struct ProverCore {
    cache_dir: PathBuf,
}

impl ProverCore {
    pub async fn new(cache_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&cache_dir)?;
        Ok(Self { cache_dir })
    }

    /// Returns the on-disk directory used for cached KZG params and circuit
    /// keys.
    pub fn cache_dir(&self) -> &std::path::Path {
        &self.cache_dir
    }
}
