//! Read-only Diagnostics panel rendering
//! `BridgeState::metrics().snapshot()`.
//!
//! Shows three sections:
//! - **Counters** — flat key → u64 (`vcs.issued`, `dids.bootstrapped`, …).
//! - **HTTP** — per-(method, host, status) histogram: count, min/p50/p95/max ms,
//!   total bytes downloaded.
//! - **Ops** — per-(op-name, outcome) histogram for the bracketed flows
//!   (`bootstrap_did`, `oid4vp_authenticate`, `issuance`, `self_verify`,
//!   `indexer.*`, `prover.prove`): same latency stats plus summed
//!   RSS-growth (KiB) + summed CPU-time (μs).
//!
//! Refreshes every render — the panel is mounted inside the
//! Diagnostics carousel, so a click on the tab triggers a fresh
//! `snapshot()` read. A "Reset stats" button clears the
//! aggregator (does NOT clear the persisted Logs archive — those
//! are independent).

use dioxus::prelude::*;

use crate::bridge::BridgeState;

#[component]
pub fn TelemetryPanel(bridge_state: BridgeState) -> Element {
    // Re-read on every render. `snapshot()` is O(samples * log
    // samples) which is fine for Phase-1 wallets (low tens of
    // ops per session). If a session ever generates thousands
    // we can memoise behind a `use_memo` tied to a refresh tick.
    let aggregator = bridge_state.metrics();
    let snap = aggregator.snapshot();

    let counters_empty = snap.counters.is_empty();
    let http_empty = snap.http.is_empty();
    let ops_empty = snap.ops.is_empty();
    let all_empty = counters_empty && http_empty && ops_empty;

    let reset = {
        let aggregator = aggregator.clone();
        move |_| aggregator.reset()
    };

    if all_empty {
        return rsx! {
            div { class: "card",
                div { class: "card-header", "Telemetry" }
                div { class: "session-log-empty",
                    "No telemetry yet. Drive a Bootstrap DID, OID4VP authenticate, \
                     Get credential, or Self-verify in the Identity Centre tab to \
                     populate latency histograms + counters."
                }
            }
        };
    }

    rsx! {
        div { class: "card",
            div { class: "card-header",
                "Telemetry"
                div { style: "flex: 1;" }
                button {
                    style: "padding: 4px 12px; font-size: 11px;",
                    onclick: reset,
                    "Reset stats"
                }
            }

            // Counters
            if !counters_empty {
                div { class: "card-section-header", "Counters" }
                div { class: "metrics-summary",
                    {snap.counters.iter().map(|(k, v)| rsx! {
                        div { class: "metric-pill",
                            span { class: "k", "{k}" }
                            span { class: "v", "{v}" }
                        }
                    })}
                }
            }

            // HTTP histograms
            if !http_empty {
                div { class: "card-section-header", "HTTP latency" }
                table { class: "metrics-table",
                    thead {
                        tr {
                            th { "Endpoint" }
                            th { "Count" }
                            th { "p50" }
                            th { "p95" }
                            th { "Max" }
                            th { "Bytes" }
                        }
                    }
                    tbody {
                        {snap.http.iter().map(|(k, h)| rsx! {
                            tr {
                                td { class: "seed-blob", "{k}" }
                                td { "{h.count}" }
                                td { "{h.p50_ms}ms" }
                                td { "{h.p95_ms}ms" }
                                td { "{h.max_ms}ms" }
                                td { "{h.total_bytes}" }
                            }
                        })}
                    }
                }
            }

            // Op histograms
            if !ops_empty {
                div { class: "card-section-header", "Op latency + RSS / CPU" }
                table { class: "metrics-table",
                    thead {
                        tr {
                            th { "Op" }
                            th { "Count" }
                            th { "p50" }
                            th { "p95" }
                            th { "Max" }
                            th { "Σ RSS Δ kB" }
                            th { "Σ CPU μs" }
                        }
                    }
                    tbody {
                        {snap.ops.iter().map(|(k, h)| rsx! {
                            tr {
                                td { class: "seed-blob", "{k}" }
                                td { "{h.count}" }
                                td { "{h.p50_ms}ms" }
                                td { "{h.p95_ms}ms" }
                                td { "{h.max_ms}ms" }
                                td { "{h.total_rss_growth_kb}" }
                                td { "{h.total_cpu_us}" }
                            }
                        })}
                    }
                }
            }
        }
    }
}
