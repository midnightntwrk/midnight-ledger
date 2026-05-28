//! `ControllerSecretService` — per-DID random controller secrets
//! (CRUD on the `CONTROLLER_SECRETS` redb table).
//!
//! Wave A1: struct + constructor only. Bodies in wave C1.

use std::sync::Arc;

use crate::store::WalletStore;

pub struct ControllerSecretService {
    pub(crate) store: Arc<WalletStore>,
}

impl ControllerSecretService {
    pub fn new(store: Arc<WalletStore>) -> Self {
        Self { store }
    }
}
