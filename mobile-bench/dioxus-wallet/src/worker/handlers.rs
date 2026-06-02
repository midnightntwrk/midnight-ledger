//! Per-message dispatch. Phase 2 / Task 1 only handles
//! [`super::WorkMsg::Noop`]; later tasks add Bootstrap, OID4VP,
//! OID4VCI, Unlock, RefreshDid, CreateDid, DeactivateDid,
//! VcSelfVerify.
//!
//! Each handler should:
//! - Take whatever it needs from the `WorkMsg` arm + any context
//!   threaded into the worker constructor.
//! - Run the actual `wallet-core` async op on the worker's
//!   tokio runtime (no special precautions; we have 8 MiB of
//!   stack and a proper Tokio runtime here).
//! - Return a [`super::WorkOutcome`] that mirrors the request's
//!   arm, with the matching `action_id` echoed back so the
//!   router can find the right handler.

use super::{WorkMsg, WorkOutcome};

/// Dispatch a [`WorkMsg`] to its handler and return the
/// [`WorkOutcome`]. Pure routing — no shared mutable state.
pub(super) async fn dispatch(msg: WorkMsg) -> WorkOutcome {
    match msg {
        WorkMsg::Noop { action_id } => {
            tracing::info!(
                target: "wallet_worker",
                action_id,
                "WorkMsg::Noop received → NoopAck",
            );
            WorkOutcome::NoopAck { action_id }
        }
    }
}
