//! Use-case service layer — originally planned as the
//! hexagonal core that consumes port traits and exposes
//! verb-shaped public methods.
//!
//! ## Status
//!
//! **These services were never populated.** The wave-A1 commit
//! shipped struct + constructor skeletons; wave C was meant to
//! migrate flows in, but the codebase took a different path:
//! the OID4VP and OID4VCI use cases ended up as **free-function
//! orchestrators** living next to their protocol modules —
//! `oid4vp_client::run_authentication` (driven by
//! `LoginCoordinator + ResponseBuilder`) and
//! `oid4vci_client::run_issuance` (driven by
//! `CredentialCoordinator + ProofBuilder`). Bootstrap is
//! `did::bootstrap_did_with_keys`. Each is "do one thing,
//! testable, port-typed" — same architectural intent as the
//! services, different shape.
//!
//! The structs in this directory are reachable from
//! `wallet-core/src/lib.rs` so existing callers that imported
//! them still compile, but none have method bodies. Fields are
//! still `Arc<dyn Port>` placeholders with `#[allow(dead_code)]`
//! suppressing the unused-field warning.
//!
//! ## What this means in practice
//!
//! - New use cases follow the **orchestrator-function +
//!   coordinator** pattern (see `oid4vci_client::proof` for the
//!   latest example). Don't add methods here.
//! - Reading the audit at
//!   `docs/superpowers/specs/2026-06-03-hex-architecture-audit.md`
//!   §3 first will save you a wrong turn.
//! - A cleanup pass that deletes these dead skeletons is
//!   warranted, but lives in a separate commit because it
//!   touches the public surface (and might affect downstream
//!   `service::*` imports that exist for forward compatibility).
//!
//! Original design spec (kept for historical context):
//! `docs/superpowers/specs/2026-05-29-hexagonal-headless-wallet-design.md`
//! §2.2.

// Stub structs hold `Arc<dyn Port>` fields they don't read.
// `#![deny(warnings)]` at the crate root would otherwise refuse
// the placeholders. See the module docstring for why these
// services never landed; the lint scope here can drop once a
// cleanup commit removes the dead surface.
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
