use std::path::{Path, PathBuf};
use std::sync::Arc;

use base_crypto::data_provider::{FetchMode, MidnightDataProvider, OutputMode};
use ledger::dust::{DUST_EXPECTED_FILES, DustResolver};
use zswap::{ZSWAP_EXPECTED_FILES, prove::ZswapResolver};

/// Wraps the existing MidnightDataProvider machinery. On first call, files
/// listed in DUST_EXPECTED_FILES / ZSWAP_EXPECTED_FILES are downloaded into
/// `dir`. Subsequent calls hit the cache.
#[allow(dead_code)] // fields read by zkir/dust modules in later tasks
pub(crate) struct ParamsCache {
    dir: PathBuf,
    pub(crate) zswap: Arc<ZswapResolver>,
    pub(crate) dust: Arc<DustResolver>,
}

impl ParamsCache {
    pub(crate) fn new(dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;

        // base_crypto::data_provider::MidnightDataProvider reads MIDNIGHT_PP
        // (see base-crypto/src/data_provider.rs:225) to choose its on-disk
        // cache root. We pin it to our caller-supplied `dir` if not already
        // set by the embedding process — letting tests override.
        if std::env::var_os("MIDNIGHT_PP").is_none() {
            // SAFETY: setting env vars is unsafe in Rust 2024; we only do this
            // once at construction and the value is a path under our control.
            unsafe {
                std::env::set_var("MIDNIGHT_PP", &dir);
            }
        }

        let zswap = ZswapResolver(MidnightDataProvider::new(
            FetchMode::OnDemand,
            OutputMode::Log,
            ZSWAP_EXPECTED_FILES.to_vec(),
        )?);
        let dust = DustResolver(MidnightDataProvider::new(
            FetchMode::OnDemand,
            OutputMode::Log,
            DUST_EXPECTED_FILES.to_owned(),
        )?);

        Ok(Self { dir, zswap: Arc::new(zswap), dust: Arc::new(dust) })
    }

    #[allow(dead_code)]
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }
}
