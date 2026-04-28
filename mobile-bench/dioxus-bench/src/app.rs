use dioxus::prelude::*;
use prover_core::ProofRun;

use crate::runner::{RunStatus, Runner, fmt_duration};

#[component]
pub fn App() -> Element {
    let mut status = use_signal(|| RunStatus::Idle);
    let mut last_run = use_signal::<Option<ProofRun>>(|| None);

    let runner = use_resource(|| async move { Runner::new().await });

    rsx! {
        link { rel: "stylesheet", href: asset!("/assets/styles.css") }
        h1 { "Midnight Proof Bench" }

        div { class: "row",
            button {
                disabled: !matches!(*status.read(), RunStatus::Idle | RunStatus::Done | RunStatus::Error(_)),
                onclick: move |_| {
                    let r = runner;
                    spawn(async move {
                        let r = r.read();
                        if let Some(Ok(r)) = r.as_ref() {
                            status.set(RunStatus::Proving("zkir-minimal-assert"));
                            match r.run_zkir().await {
                                Ok(run) => { last_run.set(Some(run)); status.set(RunStatus::Done); }
                                Err(e) => status.set(RunStatus::Error(e)),
                            }
                        }
                    });
                },
                "Run zkir example"
            }
        }

        div { class: "status", "Status: {format_status(&status.read())}" }

        if let Some(run) = last_run.read().as_ref() {
            div { class: "result",
                h3 { "Last run" }
                div { "Label:        {run.label}" }
                div { "k:            {run.k}" }
                div { "Prove time:   {fmt_duration(run.elapsed)}" }
                if let Some(v) = run.verify_elapsed {
                    div { "Verify time:  {fmt_duration(v)}" }
                }
                div {
                    "Verified:     ",
                    {run.verified.map(|b| if b { "yes" } else { "no" }).unwrap_or("n/a")}
                }
                div { "Proof size:   {run.proof_bytes.len()} B" }
            }
        }
    }
}

fn format_status(s: &RunStatus) -> String {
    match s {
        RunStatus::Idle => "idle".into(),
        RunStatus::Proving(l) => format!("proving {l}…"),
        RunStatus::Done => "done".into(),
        RunStatus::Error(e) => format!("error: {e}"),
    }
}
