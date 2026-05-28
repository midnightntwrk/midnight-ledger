//! `Oid4vciService` — credential issuance (paste offer → VC stored).
//!
//! Wave A1: struct + constructor only. Bodies in wave C8.

use std::sync::Arc;

use crate::clock::Clock;
use crate::http::HttpClient;
use crate::secret_storage::SecretStorage;
use crate::telemetry::Metrics;
use crate::vc_store::VcStorage;

pub struct Oid4vciService {
    pub(crate) http: Arc<dyn HttpClient>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) metrics: Arc<dyn Metrics>,
    pub(crate) secrets: Arc<dyn SecretStorage>,
    pub(crate) vcs: Arc<dyn VcStorage>,
}

impl Oid4vciService {
    pub fn new(
        http: Arc<dyn HttpClient>,
        clock: Arc<dyn Clock>,
        metrics: Arc<dyn Metrics>,
        secrets: Arc<dyn SecretStorage>,
        vcs: Arc<dyn VcStorage>,
    ) -> Self {
        Self { http, clock, metrics, secrets, vcs }
    }
}
