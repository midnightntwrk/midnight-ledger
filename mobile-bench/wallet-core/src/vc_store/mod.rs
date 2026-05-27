//! Per-holder Verifiable Credential storage.
//!
//! Three redb tables sharing the same `wallet.redb` file as the
//! existing wallet store:
//!
//! * `vcs`        — `vc_uri` → CBOR-serialized signed VC body
//! * `vc_openings`— `(vc_uri, claim_path)` → CBOR opening blob
//! * `vc_metadata`— `vc_uri` → display order, last-verified ts, custom labels
//!
//! All three tables are write-once for the VC body itself; only
//! metadata mutates after issuance. Generic over the VC envelope
//! shape so future credential families don't require schema
//! migration.
//!
//! Storage port: every consumer takes `&dyn VcStorage` so tests
//! can swap the redb-backed adapter for an in-memory one and
//! future native-on-device stores plug in the same way.

mod api;
#[cfg(any(test, feature = "test-support"))]
mod in_memory;
mod tables;
mod types;

pub use api::{RedbVcStore, VcStoreError};
#[cfg(any(test, feature = "test-support"))]
pub use in_memory::InMemoryVcStore;
pub use types::{StoredVc, VcMetadata, VcOpening};

/// Storage port for VC persistence. Implemented by `RedbVcStore`
/// (production on-disk) and `InMemoryVcStore` (tests + dev).
pub trait VcStorage: Send + Sync {
    fn insert_vc(&self, vc: &StoredVc) -> Result<(), VcStoreError>;
    fn get_vc(&self, vc_uri: &str) -> Result<Option<StoredVc>, VcStoreError>;
    fn insert_opening(&self, op: &VcOpening) -> Result<(), VcStoreError>;
    fn get_opening(
        &self,
        vc_uri: &str,
        claim_path: &str,
    ) -> Result<Option<VcOpening>, VcStoreError>;
    fn get_metadata(&self, vc_uri: &str) -> Result<Option<VcMetadata>, VcStoreError>;
    fn list_ordered(&self) -> Result<Vec<StoredVc>, VcStoreError>;
    fn delete_vc(&self, vc_uri: &str) -> Result<(), VcStoreError>;
    fn insert_vc_with_openings(
        &self,
        vc: &StoredVc,
        openings: &[VcOpening],
    ) -> Result<(), VcStoreError>;

    /// `update_metadata` is special: it takes a closure that
    /// mutates the metadata in place. The trait can't use
    /// generic / `impl FnOnce` because that'd lose object-safety.
    /// We pass `&mut dyn FnMut(&mut VcMetadata)` instead — the
    /// caller's closure boxes into that, which is the standard
    /// workaround.
    fn update_metadata(
        &self,
        vc_uri: &str,
        update: &mut dyn FnMut(&mut VcMetadata),
    ) -> Result<(), VcStoreError>;
}
