//! Use-case service layer — the hexagonal core that consumes the
//! port traits and exposes verb-shaped public methods.
//!
//! See `docs/superpowers/specs/2026-05-29-hexagonal-headless-wallet-design.md`
//! §2.2 for the full per-service API design and §2.3 for the port
//! catalogue these services depend on.
//!
//! Wave A1 (this commit): module skeleton only. Each service file
//! declares a `pub struct` with `Arc<dyn Port>` field placeholders +
//! a constructor `new(...)`. No methods yet — the UI continues to
//! drive the existing flows via `BridgeState` + `app_wallet_for()`.
//! Wave C migrates one flow at a time into these services.

// Stub structs hold `Arc<dyn Port>` fields they don't read yet —
// wave C populates the method bodies. The `#![deny(warnings)]` at
// the crate root would otherwise refuse these placeholders, so we
// scope a `dead_code` allow here for the duration of waves A-B.
// Wave G removes this attribute once every field has a consumer.
#![allow(dead_code)]

mod builder;
pub mod backup_service;
pub mod controller_secret_service;
pub mod did_service;
pub mod dust_sync_service;
pub mod identity_centre_service;
pub mod oid4vci_service;
pub mod oid4vp_service;
pub mod telemetry_service;
pub mod vc_verify_service;
pub mod wallet_service;

pub use builder::{BuildError, WalletServices, WalletServicesBuilder};
pub use backup_service::BackupService;
pub use controller_secret_service::ControllerSecretService;
pub use did_service::DidService;
pub use dust_sync_service::DustSyncService;
pub use identity_centre_service::IdentityCentreService;
pub use oid4vci_service::Oid4vciService;
pub use oid4vp_service::Oid4vpService;
pub use telemetry_service::TelemetryService;
pub use vc_verify_service::VcVerifyService;
pub use wallet_service::WalletService;
