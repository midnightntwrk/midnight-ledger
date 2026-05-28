//! `WalletServicesBuilder` — fluent dependency wiring per
//! `docs/superpowers/specs/2026-05-29-hexagonal-headless-wallet-design.md` §2.7.
//!
//! Manual builder, defended in the design doc: the wire graph is
//! small (~15 ports, ~10 services), it's read often when
//! onboarding, and the explicit shape gives compile-time errors on
//! missing fields once `build()` checks for `None`s.  Two call
//! sites: the dioxus `lib.rs::run()` and the new
//! `headless-wallet`'s `main.rs`.

use std::sync::Arc;

use crate::chain::{IndexerClient, NodeClient, Prover};
use crate::clock::Clock;
use crate::http::HttpClient;
use crate::secret_storage::SecretStorage;
use crate::store::WalletStore;
use crate::telemetry::{InMemoryMetrics, Metrics, ResourceProbe};
use crate::vc_store::VcStorage;

use super::{
    BackupService, ControllerSecretService, DidService, DustSyncService,
    IdentityCentreService, Oid4vciService, Oid4vpService, TelemetryService,
    VcVerifyService, WalletService,
};

/// Composite handle owned by the dioxus app and the headless
/// binary.  Cheap to clone (every field is `Arc`); a single
/// instance lives for the lifetime of the process and is passed
/// into Dioxus via `use_context_provider` (wave D) or into the
/// headless dispatcher (wave E).
#[derive(Clone)]
pub struct WalletServices {
    pub wallet: Arc<WalletService>,
    pub dust: Arc<DustSyncService>,
    pub did: Arc<DidService>,
    pub identity_centre: Arc<IdentityCentreService>,
    pub oid4vp: Arc<Oid4vpService>,
    pub oid4vci: Arc<Oid4vciService>,
    pub vc_verify: Arc<VcVerifyService>,
    pub backup: Arc<BackupService>,
    pub controller_secret: Arc<ControllerSecretService>,
    pub telemetry: Arc<TelemetryService>,
}

/// Fluent builder.  Every field is `Option<Arc<dyn Trait>>`;
/// `build()` returns `BuildError::MissingPort(name)` for the
/// first unset slot it encounters — wire errors surface
/// immediately at startup, not at the first use-case call.
#[derive(Default)]
pub struct WalletServicesBuilder {
    http: Option<Arc<dyn HttpClient>>,
    indexer: Option<Arc<dyn IndexerClient>>,
    node: Option<Arc<dyn NodeClient>>,
    prover: Option<Arc<dyn Prover>>,
    clock: Option<Arc<dyn Clock>>,
    metrics: Option<Arc<dyn Metrics>>,
    metrics_aggregator: Option<Arc<InMemoryMetrics>>,
    probe: Option<Arc<dyn ResourceProbe>>,
    secrets: Option<Arc<dyn SecretStorage>>,
    vcs: Option<Arc<dyn VcStorage>>,
    store: Option<Arc<WalletStore>>,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("missing port: {0}")]
    MissingPort(&'static str),
}

impl WalletServicesBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_http(mut self, h: Arc<dyn HttpClient>) -> Self {
        self.http = Some(h);
        self
    }
    pub fn with_indexer(mut self, i: Arc<dyn IndexerClient>) -> Self {
        self.indexer = Some(i);
        self
    }
    pub fn with_node(mut self, n: Arc<dyn NodeClient>) -> Self {
        self.node = Some(n);
        self
    }
    pub fn with_prover(mut self, p: Arc<dyn Prover>) -> Self {
        self.prover = Some(p);
        self
    }
    pub fn with_clock(mut self, c: Arc<dyn Clock>) -> Self {
        self.clock = Some(c);
        self
    }
    pub fn with_metrics(mut self, m: Arc<dyn Metrics>) -> Self {
        self.metrics = Some(m);
        self
    }
    pub fn with_metrics_aggregator(mut self, m: Arc<InMemoryMetrics>) -> Self {
        self.metrics_aggregator = Some(m);
        self
    }
    pub fn with_resource_probe(mut self, p: Arc<dyn ResourceProbe>) -> Self {
        self.probe = Some(p);
        self
    }
    pub fn with_secret_storage(mut self, s: Arc<dyn SecretStorage>) -> Self {
        self.secrets = Some(s);
        self
    }
    pub fn with_vc_storage(mut self, v: Arc<dyn VcStorage>) -> Self {
        self.vcs = Some(v);
        self
    }
    pub fn with_wallet_store(mut self, s: Arc<WalletStore>) -> Self {
        self.store = Some(s);
        self
    }

    pub fn build(self) -> Result<WalletServices, BuildError> {
        let http = self.http.ok_or(BuildError::MissingPort("http"))?;
        let indexer = self.indexer.ok_or(BuildError::MissingPort("indexer"))?;
        let node = self.node.ok_or(BuildError::MissingPort("node"))?;
        let prover = self.prover.ok_or(BuildError::MissingPort("prover"))?;
        let clock = self.clock.ok_or(BuildError::MissingPort("clock"))?;
        let metrics = self.metrics.ok_or(BuildError::MissingPort("metrics"))?;
        let metrics_aggregator = self
            .metrics_aggregator
            .ok_or(BuildError::MissingPort("metrics_aggregator"))?;
        let probe = self.probe.ok_or(BuildError::MissingPort("resource_probe"))?;
        let secrets = self.secrets.ok_or(BuildError::MissingPort("secret_storage"))?;
        let vcs = self.vcs.ok_or(BuildError::MissingPort("vc_storage"))?;
        let store = self.store.ok_or(BuildError::MissingPort("wallet_store"))?;

        let wallet = Arc::new(WalletService::new(
            http.clone(),
            indexer.clone(),
            node.clone(),
            prover.clone(),
            clock.clone(),
            metrics.clone(),
            secrets.clone(),
        ));
        let dust = Arc::new(DustSyncService::new(
            indexer.clone(),
            clock.clone(),
            metrics.clone(),
        ));
        let did = Arc::new(DidService::new(
            indexer.clone(),
            node.clone(),
            prover.clone(),
            clock.clone(),
            metrics.clone(),
            secrets.clone(),
        ));
        let identity_centre = Arc::new(IdentityCentreService::new(
            indexer.clone(),
            node.clone(),
            prover.clone(),
            clock.clone(),
            metrics.clone(),
            secrets.clone(),
        ));
        let oid4vp = Arc::new(Oid4vpService::new(
            http.clone(),
            clock.clone(),
            metrics.clone(),
            secrets.clone(),
        ));
        let oid4vci = Arc::new(Oid4vciService::new(
            http.clone(),
            clock.clone(),
            metrics.clone(),
            secrets.clone(),
            vcs.clone(),
        ));
        let vc_verify = Arc::new(VcVerifyService::new(
            clock.clone(),
            metrics.clone(),
            secrets.clone(),
            vcs.clone(),
        ));
        let backup = Arc::new(BackupService::new(store.clone()));
        let controller_secret = Arc::new(ControllerSecretService::new(store.clone()));
        let telemetry = Arc::new(TelemetryService::new(metrics_aggregator, probe));

        Ok(WalletServices {
            wallet,
            dust,
            did,
            identity_centre,
            oid4vp,
            oid4vci,
            vc_verify,
            backup,
            controller_secret,
            telemetry,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `build()` returns an error for the first missing port —
    /// proves the dependency graph is checked up-front rather
    /// than lazily at the first use-case call.
    #[test]
    fn build_reports_missing_port() {
        // `WalletServices` doesn't impl `Debug` (Arc<dyn Trait>
        // members aren't Debug), so `unwrap_err()` wouldn't
        // compile — match on the result instead.
        match WalletServicesBuilder::new().build() {
            Ok(_) => panic!("expected MissingPort error"),
            Err(BuildError::MissingPort(name)) => assert_eq!(name, "http"),
        }
    }

    // A "build with every port supplied" smoke test would live
    // here but requires stubs for all 11 trait dependencies.
    // Each stub mirrors its trait's full API surface; collecting
    // them in one place was awkward (variants like `TxError::Other`
    // don't exist — they have specific named variants), and the
    // per-service test files in `tests/usecase/` (wave F)
    // already need their own stub stack.  We pin those in wave B
    // when the missing ports (Randomness, WalletStorage,
    // UnlockGate, etc.) land and the stub catalogue solidifies.
}
