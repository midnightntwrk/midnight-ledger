//! DID picker modal used by the Identity Centre + Bootstrap tabs.
//!
//! When a wallet holds more than one DID, OID4VP authentication
//! and OID4VCI issuance both need to ask the user which identity
//! they want to act as. This module provides:
//!
//! - [`PickerState`] — state held in the parent's `Signal<Option<…>>`
//!   while the modal is up. Carries the candidate DIDs, a header
//!   label for the modal, and the continuation that runs when the
//!   user picks.
//! - [`require_did`] — guard that triggers a flow with a chosen DID.
//!   - 0 usable DIDs → invokes the supplied error handler.
//!   - 1 usable DID  → silently picks it and runs the continuation.
//!   - >1 usable DIDs → stashes the continuation in `pending_pick`
//!     so the modal renders next frame.
//! - [`DidPickerModal`] — the Dioxus component.
//!
//! Deactivated DIDs are filtered out of the candidate set entirely
//! — the demo doesn't have a story for "authenticate as a dead
//! DID" yet; surfacing them in the picker would be misleading.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use dioxus::prelude::*;

use crate::app::{DidInventoryEntry, DidInventoryStatus};

/// State the parent component holds while the picker is open.
///
/// `on_pick` is a single-shot callback — the modal moves it out
/// of the `RefCell` once invoked so a stale "click pick twice"
/// race can't double-fire. The same slot is used to allow
/// `FnOnce` (rather than `Fn`) continuations, which lets callers
/// move owned state (URLs, futures) into the resume path.
#[derive(Clone)]
pub(crate) struct PickerState {
    /// Candidate DIDs in display order. Already filtered to
    /// exclude `Deactivated` entries.
    pub(crate) dids: Vec<DidInventoryEntry>,
    /// Modal header label. Tells the user what they're about to
    /// authorise as — e.g. `"Authenticate via OID4VP"` or
    /// `"Request credential (OID4VCI)"`.
    pub(crate) label: String,
    /// Continuation invoked with the chosen DID string.
    pub(crate) on_pick: Rc<RefCell<Option<Box<dyn FnOnce(String)>>>>,
}

impl PartialEq for PickerState {
    fn eq(&self, other: &Self) -> bool {
        // Closure pointer comparison would be unsound; we match
        // on the visible data (DID list + label) which is enough
        // to dedupe React-style re-renders. The continuation is a
        // black box.
        self.dids == other.dids && self.label == other.label
    }
}

/// Guard a flow on having a chosen DID.
///
/// Behaviour summary:
///
/// | Usable DIDs | Action                                                              |
/// |-------------|---------------------------------------------------------------------|
/// | 0           | calls `on_error(msg)` and returns — `continuation` is dropped       |
/// | 1           | calls `continuation(only_did)` immediately                          |
/// | >1          | stashes the modal state in `pending_pick`; modal renders next frame |
pub(crate) fn require_did<F, E>(
    did_inventory: Signal<BTreeMap<String, DidInventoryEntry>>,
    mut pending_pick: Signal<Option<PickerState>>,
    label: impl Into<String>,
    continuation: F,
    mut on_error: E,
) where
    F: FnOnce(String) + 'static,
    E: FnMut(String) + 'static,
{
    let usable: Vec<DidInventoryEntry> = did_inventory
        .read()
        .values()
        .filter(|e| e.status != DidInventoryStatus::Deactivated)
        .cloned()
        .collect();
    match usable.len() {
        0 => {
            on_error(
                "No usable DIDs. Switch to the Bootstrap tab and \
                 mint one (or activate an existing one)."
                    .into(),
            );
        }
        1 => {
            let only = usable.into_iter().next().expect("len == 1");
            continuation(only.did);
        }
        _ => {
            pending_pick.set(Some(PickerState {
                dids: usable,
                label: label.into(),
                on_pick: Rc::new(RefCell::new(Some(Box::new(continuation)))),
            }));
        }
    }
}

/// Modal overlay. Renders when `pending_pick` is `Some`. Click on
/// a row to confirm; click the backdrop or `Cancel` to dismiss.
///
/// The component takes the `pending_pick` signal by value (it's
/// `Copy`) so the on-pick and on-cancel handlers can both write
/// `None` back into it without prop drilling more callbacks.
#[component]
pub(crate) fn DidPickerModal(pending_pick: Signal<Option<PickerState>>) -> Element {
    let snapshot = pending_pick.read().clone();
    let Some(state) = snapshot else {
        // Nothing to render — `Element` allows returning the empty
        // tree by handing back an empty rsx!.
        return rsx! {};
    };

    let close = {
        let mut pending_pick = pending_pick;
        move |_| pending_pick.set(None)
    };

    rsx! {
        // Backdrop — click anywhere outside the dialog to dismiss.
        div {
            class: "did-picker-backdrop",
            onclick: close,
        }
        // Dialog. `stopPropagation` on the dialog's own onclick so a
        // misclick inside the dialog doesn't bubble to the backdrop
        // and close the modal mid-pick.
        div {
            class: "did-picker-dialog",
            onclick: move |evt| evt.stop_propagation(),

            div { class: "did-picker-header",
                div { class: "did-picker-title", "{state.label}" }
                div { class: "did-picker-subtitle",
                    "Pick which DID to act as."
                }
            }

            div { class: "did-picker-list",
                for entry in state.dids.iter().cloned() {
                    {render_row(entry, pending_pick)}
                }
            }

            div { class: "did-picker-footer",
                button {
                    class: "did-picker-cancel",
                    onclick: close,
                    "Cancel"
                }
            }
        }
    }
}

/// Render one row in the DID list. Plain helper (not a `#[component]`)
/// because [`DidInventoryEntry`] doesn't implement the prop
/// constraints the macro requires.
fn render_row(
    entry: DidInventoryEntry,
    mut pending_pick: Signal<Option<PickerState>>,
) -> Element {
    let did = entry.did.clone();
    let pick = move |_| {
        // Grab the continuation out of the picker state, clear
        // the state (so the modal closes), then fire. Order
        // matters: we take the FnOnce out FIRST while we have the
        // borrow, then drop the borrow before setting `None` —
        // otherwise the read-then-write pattern would deadlock the
        // Dioxus signal.
        let continuation = {
            let state_opt = pending_pick.read();
            state_opt.as_ref().and_then(|s| s.on_pick.borrow_mut().take())
        };
        pending_pick.set(None);
        if let Some(cb) = continuation {
            cb(did.clone());
        }
    };

    let status_class = match entry.status {
        DidInventoryStatus::Active => "did-picker-status did-picker-status--active",
        DidInventoryStatus::Pending => "did-picker-status did-picker-status--pending",
        DidInventoryStatus::Deactivated => {
            // `require_did` already filters these out, so we
            // never reach this arm in practice. Render defensively
            // anyway in case a future caller bypasses the filter.
            "did-picker-status did-picker-status--deactivated"
        }
    };
    let status_label = entry.status.label_for_picker();

    rsx! {
        button {
            class: "did-picker-row",
            onclick: pick,
            div { class: "did-picker-row-text",
                div { class: "did-picker-row-did mono", "{truncate_did(&entry.did)}" }
                div { class: "did-picker-row-meta",
                    if let Some(vm) = entry.vm_count {
                        span { "{vm} VMs" }
                    }
                    if entry.vm_count.is_some() && entry.service_count.is_some() {
                        span { class: "did-picker-row-sep", "·" }
                    }
                    if let Some(svc) = entry.service_count {
                        span { "{svc} services" }
                    }
                }
            }
            div { class: "{status_class}", "{status_label}" }
        }
    }
}

/// Mid-truncate a DID string for the row. Keeps the prefix + a
/// short tail; full string sits in the row's `title` (todo —
/// Dioxus's `title` attr support varies, sticking with truncated
/// text only for now).
fn truncate_did(s: &str) -> String {
    const HEAD: usize = 20;
    const TAIL: usize = 6;
    let n = s.chars().count();
    if n <= HEAD + TAIL + 1 {
        return s.into();
    }
    let head: String = s.chars().take(HEAD).collect();
    let tail: String = s.chars().skip(n - TAIL).collect();
    format!("{head}…{tail}")
}

impl DidInventoryStatus {
    /// Picker-specific label. Same text as the inventory's badge
    /// but extracted here to avoid the `pub` reach across modules
    /// just for one display string.
    fn label_for_picker(&self) -> &'static str {
        match self {
            DidInventoryStatus::Pending => "Pending",
            DidInventoryStatus::Active => "Active",
            DidInventoryStatus::Deactivated => "Deactivated",
        }
    }
}
