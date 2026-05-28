//! `Oid4vpService` — SIOPv2 authentication (paste QR → id_token).
//!
//! Wave A1: struct + constructor only. Bodies in wave C7.

use std::sync::Arc;

use crate::clock::Clock;
use crate::http::HttpClient;
use crate::secret_storage::SecretStorage;
use crate::telemetry::Metrics;

pub struct Oid4vpService {
    pub(crate) http: Arc<dyn HttpClient>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) metrics: Arc<dyn Metrics>,
    pub(crate) secrets: Arc<dyn SecretStorage>,
}

impl Oid4vpService {
    pub fn new(
        http: Arc<dyn HttpClient>,
        clock: Arc<dyn Clock>,
        metrics: Arc<dyn Metrics>,
        secrets: Arc<dyn SecretStorage>,
    ) -> Self {
        Self { http, clock, metrics, secrets }
    }
}
