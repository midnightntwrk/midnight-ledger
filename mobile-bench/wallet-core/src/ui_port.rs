//! `UserInterface` port — the channel through which use-case
//! services talk to whoever is driving them.
//!
//! Three things services need from this port:
//!
//! - **Stage updates** — long-running flows like `bootstrap_did`
//!   want to surface "I'm at step 4 of 7, here's the context".
//!   The UI binds these to a progress signal; the headless CLI
//!   serialises them as JSON events on stdout; tests collect
//!   them into a `Vec` to assert on.
//! - **Outcome announcements** — terminal status the use-case
//!   wants the driver to *know* the operator saw. Distinct from
//!   `Notifications` (fire-and-forget): outcomes are part of
//!   the use-case's result, not a separate toast stream.
//! - **Prompts** — `prompt_text`, `prompt_passphrase`, `confirm`.
//!   Async because they block on user action. The Dioxus adapter
//!   mounts modals; the CLI adapter reads stdin; the test
//!   adapter dequeues scripted answers.
//!
//! See design doc §2.5.
//!
//! Wave B6 (this commit): trait + test adapter. The CLI adapter
//! lands in wave E inside the `headless-wallet` crate (it
//! needs the JSON wire format defined there). The Dioxus
//! adapter lands in wave D as part of the `BridgeState` →
//! service-context migration.

use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::Value;

/// Outcome attached to a `report_outcome` call.  Three flavours
/// matching the typical "did the user-visible thing work?"
/// question: Ok (with a brief message), Warn, Err.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiOutcome {
    Ok(String),
    Warn(String),
    Err(String),
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum UiError {
    #[error("user cancelled")]
    Cancelled,
    #[error("input source closed")]
    Closed,
}

/// One captured event from the test adapter — recorded for
/// later assertion.  Composite over the three event shapes
/// (stage / outcome / prompt-answered) so tests can iterate
/// a single flat list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEvent {
    Stage {
        verb: String,
        stage: String,
        data: Value,
    },
    Outcome {
        verb: String,
        outcome: UiOutcome,
    },
    /// A prompt was answered.  Captures the prompt + the
    /// answer that was dequeued from the scripted set.
    PromptAnswered {
        prompt: String,
        answer: String,
    },
    /// A prompt failed because the scripted answer queue was
    /// empty.  Captured rather than ignored so tests can
    /// detect under-scripting.
    PromptStarved {
        prompt: String,
    },
}

/// The port.  `async` only on the prompt methods — they may
/// block; everything else is synchronous push.
#[async_trait]
pub trait UserInterface: Send + Sync + 'static {
    /// Emit a stage update with arbitrary structured context.
    /// Used by long-running flows; cheap, non-blocking.
    fn report_stage(&self, verb: &str, stage: &str, data: Value);

    /// Surface a non-fatal user-visible event tied to a verb's
    /// outcome.  Distinct from `Notifications::notify` (which
    /// is a separate fire-and-forget channel) because outcomes
    /// are PART of the use-case's result — the driver needs to
    /// know the user saw them.
    fn report_outcome(&self, verb: &str, outcome: UiOutcome);

    /// Prompt for free-form text (URL, label, etc).  Blocks
    /// until the driver supplies an answer or input closes.
    async fn prompt_text(&self, prompt: &str) -> Result<String, UiError>;

    /// Prompt for a passphrase.  Same shape as `prompt_text`;
    /// adapters can mask input differently.
    async fn prompt_passphrase(&self, prompt: &str) -> Result<String, UiError>;

    /// Yes/no confirm.
    async fn confirm(&self, prompt: &str) -> Result<bool, UiError>;
}

/// Test adapter — collects every event into a flat `Vec<UiEvent>`
/// and dequeues scripted answers for prompts.  Empty answer
/// queue is recorded as `UiEvent::PromptStarved` and the prompt
/// returns `UiError::Closed`.
#[derive(Default)]
pub struct TestUiAdapter {
    inner: Mutex<TestUiState>,
}

#[derive(Default)]
struct TestUiState {
    events: Vec<UiEvent>,
    text_answers: std::collections::VecDeque<String>,
    passphrase_answers: std::collections::VecDeque<String>,
    confirm_answers: std::collections::VecDeque<bool>,
}

impl TestUiAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Script an answer for the next `prompt_text` call.  FIFO.
    pub fn push_text(&self, answer: impl Into<String>) {
        if let Ok(mut g) = self.inner.lock() {
            g.text_answers.push_back(answer.into());
        }
    }

    pub fn push_passphrase(&self, answer: impl Into<String>) {
        if let Ok(mut g) = self.inner.lock() {
            g.passphrase_answers.push_back(answer.into());
        }
    }

    pub fn push_confirm(&self, answer: bool) {
        if let Ok(mut g) = self.inner.lock() {
            g.confirm_answers.push_back(answer);
        }
    }

    /// Snapshot the recorded events without draining.  Used for
    /// assertions like "the bootstrap-did verb emitted the
    /// expected 7 stages in order".
    pub fn events(&self) -> Vec<UiEvent> {
        self.inner.lock().map(|g| g.events.clone()).unwrap_or_default()
    }

    /// Drain the events.  Useful between test phases when the
    /// caller wants to assert on just the most recent batch.
    pub fn take_events(&self) -> Vec<UiEvent> {
        self.inner
            .lock()
            .map(|mut g| std::mem::take(&mut g.events))
            .unwrap_or_default()
    }

    /// `true` if every scripted answer queue is empty — i.e.
    /// the test consumed exactly the answers it scripted.
    /// Calling this at test cleanup catches under-scripting.
    pub fn prompts_drained(&self) -> bool {
        self.inner
            .lock()
            .map(|g| {
                g.text_answers.is_empty()
                    && g.passphrase_answers.is_empty()
                    && g.confirm_answers.is_empty()
            })
            .unwrap_or(false)
    }

    /// Convenience: assert a specific (verb, stage) pair appears
    /// at least once in the captured events.  Returns the
    /// event's data payload for further checks; panics with a
    /// useful message if missing.
    pub fn expect_stage(&self, verb: &str, stage: &str) -> Value {
        let events = self.events();
        for ev in &events {
            if let UiEvent::Stage {
                verb: v,
                stage: s,
                data,
            } = ev
            {
                if v == verb && s == stage {
                    return data.clone();
                }
            }
        }
        panic!(
            "expected stage {verb}:{stage} not found in {} events:\n{:#?}",
            events.len(),
            events,
        );
    }
}

#[async_trait]
impl UserInterface for TestUiAdapter {
    fn report_stage(&self, verb: &str, stage: &str, data: Value) {
        if let Ok(mut g) = self.inner.lock() {
            g.events.push(UiEvent::Stage {
                verb: verb.to_string(),
                stage: stage.to_string(),
                data,
            });
        }
    }

    fn report_outcome(&self, verb: &str, outcome: UiOutcome) {
        if let Ok(mut g) = self.inner.lock() {
            g.events.push(UiEvent::Outcome {
                verb: verb.to_string(),
                outcome,
            });
        }
    }

    async fn prompt_text(&self, prompt: &str) -> Result<String, UiError> {
        match self.inner.lock() {
            Ok(mut g) => {
                if let Some(ans) = g.text_answers.pop_front() {
                    g.events.push(UiEvent::PromptAnswered {
                        prompt: prompt.to_string(),
                        answer: ans.clone(),
                    });
                    Ok(ans)
                } else {
                    g.events.push(UiEvent::PromptStarved {
                        prompt: prompt.to_string(),
                    });
                    Err(UiError::Closed)
                }
            }
            Err(_) => Err(UiError::Closed),
        }
    }

    async fn prompt_passphrase(&self, prompt: &str) -> Result<String, UiError> {
        match self.inner.lock() {
            Ok(mut g) => {
                if let Some(ans) = g.passphrase_answers.pop_front() {
                    g.events.push(UiEvent::PromptAnswered {
                        prompt: prompt.to_string(),
                        // Don't surface the passphrase in the
                        // event log — that's an obvious footgun
                        // for tests that print events on failure.
                        answer: "<redacted>".to_string(),
                    });
                    Ok(ans)
                } else {
                    g.events.push(UiEvent::PromptStarved {
                        prompt: prompt.to_string(),
                    });
                    Err(UiError::Closed)
                }
            }
            Err(_) => Err(UiError::Closed),
        }
    }

    async fn confirm(&self, prompt: &str) -> Result<bool, UiError> {
        match self.inner.lock() {
            Ok(mut g) => {
                if let Some(ans) = g.confirm_answers.pop_front() {
                    g.events.push(UiEvent::PromptAnswered {
                        prompt: prompt.to_string(),
                        answer: if ans { "yes".into() } else { "no".into() },
                    });
                    Ok(ans)
                } else {
                    g.events.push(UiEvent::PromptStarved {
                        prompt: prompt.to_string(),
                    });
                    Err(UiError::Closed)
                }
            }
            Err(_) => Err(UiError::Closed),
        }
    }
}

/// Null-object adapter — accepts everything, returns
/// `UiError::Closed` on every prompt.  Useful when a service is
/// invoked from a context that doesn't have a real UI driver
/// (e.g. some background sync paths).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopUiAdapter;

#[async_trait]
impl UserInterface for NoopUiAdapter {
    fn report_stage(&self, _: &str, _: &str, _: Value) {}
    fn report_outcome(&self, _: &str, _: UiOutcome) {}
    async fn prompt_text(&self, _: &str) -> Result<String, UiError> {
        Err(UiError::Closed)
    }
    async fn prompt_passphrase(&self, _: &str) -> Result<String, UiError> {
        Err(UiError::Closed)
    }
    async fn confirm(&self, _: &str) -> Result<bool, UiError> {
        Err(UiError::Closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn report_stage_collected_in_order() {
        let ui = TestUiAdapter::new();
        ui.report_stage("bootstrap", "step.1", json!({"n": 1}));
        ui.report_stage("bootstrap", "step.2", json!({"n": 2}));
        let events = ui.events();
        assert_eq!(events.len(), 2);
        match &events[0] {
            UiEvent::Stage { stage, .. } => assert_eq!(stage, "step.1"),
            other => panic!("expected Stage, got {other:?}"),
        }
    }

    #[test]
    fn expect_stage_finds_matching_event() {
        let ui = TestUiAdapter::new();
        ui.report_stage("bootstrap", "vk.load", json!({"circuit": "setVM"}));
        let data = ui.expect_stage("bootstrap", "vk.load");
        assert_eq!(data["circuit"], "setVM");
    }

    #[test]
    #[should_panic(expected = "expected stage bootstrap:missing")]
    fn expect_stage_panics_when_missing() {
        let ui = TestUiAdapter::new();
        ui.report_stage("bootstrap", "vk.load", json!({}));
        let _ = ui.expect_stage("bootstrap", "missing");
    }

    #[tokio::test]
    async fn prompt_dequeues_scripted_answer() {
        let ui = TestUiAdapter::new();
        ui.push_text("alice");
        ui.push_text("bob");
        let a = ui.prompt_text("name").await.unwrap();
        let b = ui.prompt_text("name").await.unwrap();
        assert_eq!(a, "alice");
        assert_eq!(b, "bob");
        assert!(ui.prompts_drained());
    }

    #[tokio::test]
    async fn prompt_starves_when_queue_empty() {
        let ui = TestUiAdapter::new();
        let err = ui.prompt_text("name").await.unwrap_err();
        assert_eq!(err, UiError::Closed);
        // The starvation was recorded.
        let events = ui.events();
        assert!(matches!(
            events.last(),
            Some(UiEvent::PromptStarved { .. })
        ));
    }

    #[tokio::test]
    async fn passphrase_prompt_redacts_event_log() {
        let ui = TestUiAdapter::new();
        ui.push_passphrase("hunter2");
        let _ = ui.prompt_passphrase("pw").await.unwrap();
        let events = ui.events();
        match &events[0] {
            UiEvent::PromptAnswered { answer, .. } => {
                assert_eq!(answer, "<redacted>");
            }
            other => panic!("expected PromptAnswered, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn confirm_dequeues_bool() {
        let ui = TestUiAdapter::new();
        ui.push_confirm(true);
        ui.push_confirm(false);
        assert!(ui.confirm("a").await.unwrap());
        assert!(!ui.confirm("b").await.unwrap());
    }

    #[tokio::test]
    async fn prompts_drained_detects_residue() {
        let ui = TestUiAdapter::new();
        ui.push_text("unused");
        assert!(!ui.prompts_drained());
        let _ = ui.prompt_text("x").await.unwrap();
        assert!(ui.prompts_drained());
    }

    #[tokio::test]
    async fn noop_adapter_returns_closed_on_prompts() {
        let ui = NoopUiAdapter;
        assert_eq!(ui.prompt_text("x").await.unwrap_err(), UiError::Closed);
        assert_eq!(
            ui.prompt_passphrase("x").await.unwrap_err(),
            UiError::Closed
        );
        assert_eq!(ui.confirm("x").await.unwrap_err(), UiError::Closed);
    }

    #[test]
    fn take_events_drains() {
        let ui = TestUiAdapter::new();
        ui.report_stage("v", "s", json!(1));
        ui.report_stage("v", "s", json!(2));
        let first = ui.take_events();
        assert_eq!(first.len(), 2);
        let second = ui.take_events();
        assert_eq!(second.len(), 0);
    }
}
