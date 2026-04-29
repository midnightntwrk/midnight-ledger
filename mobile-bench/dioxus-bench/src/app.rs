use dioxus::prelude::*;
use prover_core::ProofRun;

use crate::runner::{RunStatus, Runner, fmt_duration};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Surface {
    Zkir,
    Htc,
    Ec,
}

impl Surface {
    fn label(self) -> &'static str {
        match self {
            Surface::Zkir => "zkir-minimal-assert",
            Surface::Htc => "zkir-hash-to-curve",
            Surface::Ec => "zkir-ec-mul-add",
        }
    }

    fn button_text(self) -> &'static str {
        match self {
            Surface::Zkir => "Run zkir example",
            Surface::Htc => "Run hash-to-curve",
            Surface::Ec => "Run ec_mul + ec_add",
        }
    }
}

#[component]
pub fn App() -> Element {
    let mut status = use_signal(|| RunStatus::Idle);
    let mut last_run = use_signal::<Option<ProofRun>>(|| None);

    let runner = use_resource(|| async move { Runner::new().await });

    let run_surface = move |surface: Surface| {
        let r = runner;
        spawn(async move {
            let r = r.read();
            if let Some(Ok(r)) = r.as_ref() {
                status.set(RunStatus::Proving(surface.label()));
                let res = match surface {
                    Surface::Zkir => r.run_zkir().await,
                    Surface::Htc => r.run_htc().await,
                    Surface::Ec => r.run_ec().await,
                };
                match res {
                    Ok(run) => {
                        last_run.set(Some(run));
                        status.set(RunStatus::Done);
                    }
                    Err(e) => status.set(RunStatus::Error(e)),
                }
            }
        });
    };

    let busy = !matches!(
        *status.read(),
        RunStatus::Idle | RunStatus::Done | RunStatus::Error(_)
    );

    rsx! {
        link { rel: "stylesheet", href: asset!("/assets/styles.css") }
        h1 { "Midnight Proof Bench" }

        div { class: "row",
            for surface in [Surface::Zkir, Surface::Htc, Surface::Ec] {
                button {
                    key: "{surface.label()}",
                    disabled: busy,
                    onclick: move |_| run_surface(surface),
                    "{surface.button_text()}"
                }
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
