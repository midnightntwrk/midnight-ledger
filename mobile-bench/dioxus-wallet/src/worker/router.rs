//! Map `action_id → OutcomeHandler` so the Dioxus-side outcome
//! pump can dispatch each [`super::WorkOutcome`] back to the
//! click handler that originated it. The handler is a `FnOnce`
//! held in a `Mutex<HashMap>`; the pump `.take()`s it as it
//! consumes the outcome.
//!
//! Multi-shot wizards (Create-DID's `WizardStage` stream, the
//! batch op runner) need progress events delivered before the
//! terminal outcome. Phase 3 will extend this with a
//! `Box<dyn Fn(...) -> ControlFlow<()>>` variant, but the v1
//! `FnOnce` shape covers every current Bootstrap / OID4* / Unlock
//! / DID-refresh / Deactivate case, which all emit exactly one
//! outcome.

use std::collections::HashMap;
use std::sync::Mutex;

use super::WorkOutcome;

/// `FnOnce` because every currently-migrated handler fires
/// exactly once. `Send` so the outcome pump (which is `Send`
/// even though Dioxus signals are scope-bound) can move it
/// across the await boundary; `'static` because we hold these in
/// a process-global registry.
pub type OutcomeHandler = Box<dyn FnOnce(WorkOutcome) + Send + 'static>;

#[derive(Default)]
pub struct OutcomeRouter {
    pending: Mutex<HashMap<u64, OutcomeHandler>>,
}

impl OutcomeRouter {
    /// Register a handler keyed by `action_id`. The click site
    /// should call this *before* sending the matching `WorkMsg`
    /// so a fast worker can't race the registration.
    ///
    /// Re-registering an existing `action_id` is a programming
    /// bug — we log and drop the old handler. `action_id` is
    /// `next_action_id`-allocated so legitimate collisions can't
    /// happen.
    pub fn register(&self, action_id: u64, handler: OutcomeHandler) {
        let mut guard = self.pending.lock().expect("OutcomeRouter mutex poisoned");
        if guard.insert(action_id, handler).is_some() {
            tracing::warn!(
                target: "wallet_worker",
                action_id,
                "OutcomeRouter::register replaced an existing handler — duplicate action_id?",
            );
        }
    }

    /// Pull the handler matching `action_id`, removing it from
    /// the registry. Called by the outcome pump exactly once per
    /// outcome.
    ///
    /// Returns `None` if the click site forgot to register, or
    /// if the matching click was cancelled (component unmounted
    /// before the outcome arrived). The outcome pump treats
    /// `None` as "drop on the floor + log".
    pub fn take(&self, action_id: u64) -> Option<OutcomeHandler> {
        self.pending
            .lock()
            .expect("OutcomeRouter mutex poisoned")
            .remove(&action_id)
    }
}
