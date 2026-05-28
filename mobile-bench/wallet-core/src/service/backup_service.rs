//! `BackupService` — export / import wallet seeds + controller
//! secrets through the existing `store::backup` infrastructure
//! (R-series commit fc83f878).
//!
//! Wave A1: struct + constructor only. Bodies in wave C10.

use std::sync::Arc;

use crate::store::WalletStore;

pub struct BackupService {
    pub(crate) store: Arc<WalletStore>,
}

impl BackupService {
    pub fn new(store: Arc<WalletStore>) -> Self {
        Self { store }
    }
}
