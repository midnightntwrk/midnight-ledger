//! Map `action_id → OutcomeHandler` so the Dioxus-side outcome
//! pump can dispatch each [`super::WorkOutcome`] back to the
//! click handler that originated it.
//!
//! Lives in a `thread_local!` because the registered handlers
//! capture Dioxus `Signal`s + `EventHandler`s, which are `!Send`
//! (they hold `Rc` / `RefCell` to scope-bound storage). Putting
//! them in an `Arc<Mutex<HashMap>>` would force a `Send` bound
//! we can't satisfy.
//!
//! ## Thread-affinity invariant
//!
//! Both [`register`] and [`take`] **must run on the same OS
//! thread** — the one the outcome pump (`use_future` in
//! `App::run`) executes on, which is also the thread Dioxus'
//! `spawn` schedules tasks onto. Empirically (on the
//! Pixel-Fold-API-35 emulator), the raw onclick event handler
//! runs on a *different* thread than the pump's `use_future` —
//! a registration done directly from an onclick body lands in
//! the wrong `thread_local` and the subsequent `take` from the
//! pump returns `None`, surfacing as "outcome dropped — no
//! registered handler".
//!
//! Callers must therefore wrap their `register` + `worker.send`
//! pair in `spawn(async { … })` so the work runs on Dioxus' own
//! task pool. Every existing click site does this; new sites
//! should follow the same pattern. The `Worker Task 5` re-attempt
//! commit (this commit) is the canonical example —
//! `app::App::run::on_unlock`.
//!
//! The worker thread never reaches into this module; it only
//! emits outcomes onto the channel.

use std::cell::RefCell;
use std::collections::HashMap;

use super::WorkOutcome;

/// `FnOnce` because every currently-migrated handler fires
/// exactly once. No `Send` bound — Dioxus Signals are `!Send`,
/// and this storage lives on the UI thread.
pub type OutcomeHandler = Box<dyn FnOnce(WorkOutcome)>;

thread_local! {
    static PENDING: RefCell<HashMap<u64, OutcomeHandler>> =
        RefCell::new(HashMap::new());
}

/// Register a handler keyed by `action_id`. Call this *before*
/// sending the matching `WorkMsg` so a fast worker can't race
/// the registration.
///
/// Re-registering an existing `action_id` is a programming bug
/// — we log and drop the old handler. `next_action_id` allocation
/// is monotonic per process, so legitimate collisions can't
/// happen.
pub fn register(action_id: u64, handler: OutcomeHandler) {
    PENDING.with(|p| {
        if p.borrow_mut().insert(action_id, handler).is_some() {
            tracing::warn!(
                target: "wallet_worker",
                action_id,
                "OutcomeRouter::register replaced an existing handler — duplicate action_id?",
            );
        }
    });
}

/// Pull the handler matching `action_id`, removing it from the
/// registry. Called by the outcome pump exactly once per
/// outcome.
///
/// Returns `None` if the click site forgot to register, or if
/// the matching click was cancelled (component unmounted before
/// the outcome arrived). The outcome pump treats `None` as
/// "drop on the floor + log".
pub fn take(action_id: u64) -> Option<OutcomeHandler> {
    PENDING.with(|p| p.borrow_mut().remove(&action_id))
}
