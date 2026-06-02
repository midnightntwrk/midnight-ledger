//! Response builders — chain-of-responsibility pieces that
//! together fill an [`AuthorizationResponse`].
//!
//! Phase 1 ships just one ([`IdTokenBuilder`]); Phase 2 will add
//! `VpTokenBuilder` + `PresentationSubmissionBuilder` as sibling
//! files. The [`super::LoginCoordinator`] (Task 6) walks a
//! `Vec<Box<dyn ResponseBuilder>>` in registration order; each
//! builder reads the current `AuthorizationResponse` and may
//! depend on fields earlier builders populated.
//!
//! ## Why a trait + coordinator instead of a flat function
//!
//! Mode-A login is 4 inline lines today; the coordinator looks
//! over-engineered for it. The point is Mode B (id_token +
//! vp_token) and Mode C (vp_token only) — Phase 2 adds two more
//! builders and **does not touch** any existing Phase-1 file.
//! That's the architectural payoff: extension by registration,
//! not by editing the orchestrator.

use async_trait::async_trait;

use super::errors::LoginError;
use super::request::AuthorizationRequest;
use super::response::AuthorizationResponse;

/// Contribute to the authorization response. Builders run in
/// the order they're registered on the [`super::LoginCoordinator`];
/// later builders may depend on fields earlier ones populated
/// (e.g. a Phase-2 `PresentationSubmissionBuilder` needs
/// `VpTokenBuilder`'s output).
#[async_trait]
pub trait ResponseBuilder: Send + Sync {
    /// Mutate `resp` to add this builder's contribution. The
    /// `AuthorizationRequest` is shared read-only — every
    /// builder sees the same issuer-side spec.
    async fn build(
        &self,
        req: &AuthorizationRequest,
        resp: &mut AuthorizationResponse,
    ) -> Result<(), LoginError>;
}

mod id_token_builder;
pub use id_token_builder::IdTokenBuilder;
