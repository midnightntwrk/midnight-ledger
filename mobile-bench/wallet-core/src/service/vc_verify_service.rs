//! `VcVerifyService` — self-verify cached VCs + list + mark revoked.
//!
//! Wave A1: struct + constructor only. Bodies in wave C9.

use std::sync::Arc;

use crate::clock::Clock;
use crate::secret_storage::SecretStorage;
use crate::telemetry::Metrics;
use crate::vc_store::VcStorage;

pub struct VcVerifyService {
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) metrics: Arc<dyn Metrics>,
    pub(crate) secrets: Arc<dyn SecretStorage>,
    pub(crate) vcs: Arc<dyn VcStorage>,
}

impl VcVerifyService {
    pub fn new(
        clock: Arc<dyn Clock>,
        metrics: Arc<dyn Metrics>,
        secrets: Arc<dyn SecretStorage>,
        vcs: Arc<dyn VcStorage>,
    ) -> Self {
        Self { clock, metrics, secrets, vcs }
    }
}
