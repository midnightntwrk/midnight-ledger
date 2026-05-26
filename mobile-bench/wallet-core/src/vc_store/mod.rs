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

mod api;
mod tables;
mod types;

pub use api::VcStore;
pub use types::{StoredVc, VcMetadata, VcOpening};
