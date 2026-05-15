//! DUST spend prover artifacts.
//!
//! `MidnightDataProvider` ships per-artifact SHA-256 hashes baked
//! into `ledger::dust::DUST_EXPECTED_FILES`. The real bytes
//! (`spend.prover` / `spend.verifier` / `spend.bzkir`) get fetched
//! from `$MIDNIGHT_PARAM_SOURCE` (default
//! `https://srs.midnight.network/`) on first use and cached at
//! `$MIDNIGHT_PP` / `$XDG_CACHE_HOME/midnight/zk-params` /
//! `$HOME/.cache/midnight/zk-params`. The cache is shared with
//! every other midnight tool — dev machines that already ran the
//! upstream test suite have the artifacts in place.

use base_crypto::data_provider::{FetchMode, MidnightDataProvider, OutputMode};
use ledger::dust::{DUST_EXPECTED_FILES, DustResolver};

/// Build a `DustResolver` pointing at the standard cache dir.
/// First call on a fresh machine triggers the one-time download.
#[allow(dead_code)] // Wired by tx::prove in Task 9.
pub(crate) fn dust_resolver() -> std::io::Result<DustResolver> {
    let provider = MidnightDataProvider::new(
        FetchMode::OnDemand,
        OutputMode::Log,
        DUST_EXPECTED_FILES.to_owned(),
    )?;
    Ok(DustResolver(provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity: the resolver constructs without panicking.
    /// OnDemand mode only fetches when keys are actually resolved,
    /// so this stays offline.
    #[test]
    fn dust_resolver_constructs() {
        let _r = dust_resolver().expect("constructs");
    }
}
