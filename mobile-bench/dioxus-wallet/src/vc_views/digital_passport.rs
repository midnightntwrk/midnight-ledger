//! `digital-passport:v1` credential card.
//!
//! Renders a `StoredVc` whose schema matches the upstream
//! `midnight-verifiable-credentials/.../credential-families/digital-passport`
//! family. The card has three claim rows — `firstName`, `lastName`,
//! `dateOfBirth` — each tagged with its privacy tier:
//!
//! | Claim         | Tier                                  | Disclosure                    |
//! |---------------|---------------------------------------|-------------------------------|
//! | `firstName`   | `committedPrivate`                    | Reveal opens commitment       |
//! | `lastName`    | `committedPrivate`                    | Reveal opens commitment       |
//! | `dateOfBirth` | `committedPrivate` + `predicateOnly`  | Age-over-threshold predicate  |
//!
//! Per-tier visual language:
//!
//! - 🔒 **Hidden** (default): claim commitment only; chip rendered
//!   in muted text.
//! - 👁 **Revealed**: when the user toggles Reveal, the opening's
//!   `plaintext` is decoded as UTF-8 (text-padded, null-stripped)
//!   and displayed verbatim. This is what the verifier would receive
//!   in the eventual VP flow — Phase 1 does no on-screen redaction.
//! - 🧮 **Predicate-only**: `dateOfBirth` is never revealed; the
//!   only disclosable fact about it is "holder is ≥ N years old"
//!   for a chosen threshold. The dropdown selects N; Phase 1 does
//!   not yet emit the predicate proof on click — that lands with
//!   the full VP flow.
//!
//! # Extraction
//!
//! No `BridgeState` / `Network` types are imported. The component
//! receives an opening fetcher closure so that the redb-backed
//! `RedbVcStore` plumbing stays at the dispatch site. When this
//! module moves into the upstream VC repo, the only adjustment is
//! the `StoredVc` import path (currently `wallet_core::StoredVc`,
//! upstream would be the equivalent VC envelope type).

use dioxus::prelude::*;

use wallet_core::{StoredVc, VcOpening};

/// Upstream schema identifier — see
/// `credential-families/digital-passport/README.md`.
pub const SCHEMA_ID: &str = "digital-passport:v1";

/// Upstream package identifier — same source.
///
/// Kept as a public constant alongside `SCHEMA_ID` so future
/// callers can build full `schemaRef` records without retyping
/// the string. Unused inside this module today.
#[allow(dead_code)]
pub const PACKAGE_ID: &str = "midnight:vc:digital-passport";

/// Match a `StoredVc` to the digital-passport family.
///
/// **Phase 1 demo heuristic:** matches by `vc_uri` prefix
/// `urn:vc:digital-passport:`. The dev-only sample insertion
/// (Identity Centre, debug builds) emits this prefix and seeds the
/// three openings under their canonical JSON-Pointer paths. In
/// production the matcher would CBOR-decode `vc.body` and inspect
/// the `schemaRef.schemaId` field — landing alongside the upstream
/// extraction.
pub fn is_digital_passport(vc: &StoredVc) -> bool {
    vc.vc_uri.starts_with("urn:vc:digital-passport:")
        || vc.vc_uri.contains(":digital-passport:")
}

/// JSON-Pointer-style claim paths for the three credentialSubject
/// fields. Must match the keys the issuer emits when inserting
/// openings via `VcStorage::insert_opening`.
pub const CLAIM_FIRST_NAME: &str = "/credentialSubject/firstName";
pub const CLAIM_LAST_NAME: &str = "/credentialSubject/lastName";
pub const CLAIM_DATE_OF_BIRTH: &str = "/credentialSubject/dateOfBirth";

/// Decode a `Bytes<64>` text-padded claim opening into its UTF-8
/// payload. The upstream encoder right-pads with NUL bytes; we
/// strip the run of trailing zeros and decode the remainder.
/// Lossy on non-UTF-8 inputs — those would never appear in
/// `digital-passport` text claims, but a corrupt store shouldn't
/// crash the UI.
pub fn decode_text_padded(plaintext: &[u8]) -> String {
    let end = plaintext
        .iter()
        .rposition(|b| *b != 0)
        .map(|i| i + 1)
        .unwrap_or(0);
    String::from_utf8_lossy(&plaintext[..end]).into_owned()
}

/// Decode a `Uint<32>` days-since-epoch claim opening into a YMD
/// string. Returns `"<n days>"` when the encoded length doesn't
/// match a 4-byte little-endian payload — Phase 1 hides this
/// claim by default (predicate-only) so the display path is
/// currently only exercised by tests; kept public so the Phase 2
/// "computed age" preview (when the user opts in to seeing what
/// their selected threshold actually proves) can call it directly.
#[allow(dead_code)]
pub fn decode_days_since_epoch(plaintext: &[u8]) -> String {
    if plaintext.len() != 4 {
        return format!("<{} bytes>", plaintext.len());
    }
    let days = u32::from_le_bytes([
        plaintext[0],
        plaintext[1],
        plaintext[2],
        plaintext[3],
    ]) as i64;
    // 1970-01-01 + N days. Naive Julian arithmetic suffices for
    // the demo window (no negative years, no pre-1970 birthdays).
    let unix_secs = days.saturating_mul(86_400);
    format_unix_date_ymd(unix_secs)
}

/// Convert a Unix timestamp (seconds) to `YYYY-MM-DD` using a
/// simple algorithm — avoids pulling in `chrono` just for this
/// one renderer. Source: Howard Hinnant's date routines.
fn format_unix_date_ymd(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = (days - era * 146_097) as i64;
    let yoe =
        (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Common age thresholds. The modern card switched from a
/// `<select>` dropdown to a free-form `<input type="number">`
/// so this list is no longer rendered, but it's kept as
/// documentation of the values a verifier is most likely to
/// request (and the BDD harness uses it).
#[allow(dead_code)]
const AGE_THRESHOLDS: &[u32] = &[13, 16, 18, 21, 65];

/// Digital-Passport card. Self-contained so the entire component
/// can move into the upstream VC repository without rewiring.
///
/// Inputs:
/// - `vc`: the stored credential envelope (no decoding done here)
/// - `opening_first` / `opening_last` / `opening_dob`: eagerly-
///   resolved opening blobs the host pulled from its store. `None`
///   for any of them means "no opening recorded" — the card
///   renders that row with a "cannot reveal" caption.
/// - `reveal_first` / `reveal_last`: whether each selectively-
///   disclosable claim is currently shown in plaintext. Host owns
///   the state; the card calls back via `on_toggle_first` /
///   `on_toggle_last` when the user clicks Reveal / Hide.
/// - `age_threshold` / `on_threshold_change`: same pattern for the
///   predicate-only DOB row's age threshold dropdown.
/// - `verify_label`: optional pre-computed self-verify badge text
///   from the host — `None` ⇒ the card omits the badge slot.
/// - `on_delete`: optional handler invoked with the `vc_uri` when
///   the user clicks Delete. `None` ⇒ the Delete button is
///   omitted (useful when the host doesn't want a destructive
///   action on this row).
///
/// **No hooks inside.** All per-row state is owned by the host so
/// the card stays a pure presentation function — safe to invoke
/// from inside a Dioxus `for` loop where the row count can change
/// across renders.
///
/// No interactions emit network or chain ops yet; the Reveal
/// toggles and the threshold dropdown route through callbacks.
/// Phase 2 will wire `Generate presentation` to the OID4VP client.
///
/// The PascalCase name matches Dioxus's component convention. We
/// don't use the `#[component]` macro because `StoredVc` doesn't
/// derive `PartialEq` — the macro generates a `Props` struct that
/// requires `PartialEq`. Calling sites use the function directly.
#[allow(non_snake_case, clippy::too_many_arguments)]
pub fn DigitalPassportCard(
    vc: StoredVc,
    opening_first: Option<VcOpening>,
    opening_last: Option<VcOpening>,
    opening_dob: Option<VcOpening>,
    reveal_first: bool,
    reveal_last: bool,
    age_threshold: u32,
    on_toggle_first: EventHandler<()>,
    on_toggle_last: EventHandler<()>,
    on_threshold_change: EventHandler<u32>,
    verify_label: Option<String>,
    on_delete: Option<EventHandler<String>>,
) -> Element {
    let first_name_revealed = if reveal_first {
        opening_first
            .as_ref()
            .map(|o| decode_text_padded(&o.plaintext))
            .unwrap_or_else(|| "(opening missing)".into())
    } else {
        String::new()
    };
    let last_name_revealed = if reveal_last {
        opening_last
            .as_ref()
            .map(|o| decode_text_padded(&o.plaintext))
            .unwrap_or_else(|| "(opening missing)".into())
    } else {
        String::new()
    };

    let issuer_short = truncate(&vc.issuer_did, 24);
    let uri_short = truncate(&vc.vc_uri, 30);
    let issued_iso = format_unix_date_ymd((vc.issued_at_ms / 1000) as i64);

    let first_name_present = opening_first.is_some();
    let last_name_present = opening_last.is_some();
    let dob_present = opening_dob.is_some();

    // Rendered structure mirrors `/tmp/vc-bundle/vc_inventory_modern.html`
    // verbatim — same element nesting, same class names. The
    // matching CSS lives under `.credential-card`, `.hero`,
    // `.meta-grid`, `.claim-card`, … in `assets/styles.css`. Keep
    // the two in lockstep when iterating on the design.
    rsx! {
        section { class: "credential-card",

            // Ambient gradient blobs. Decorative only — pointer
            // events disabled in CSS so they never interfere with
            // clicks on the actual content layered above.
            div { class: "ambient ambient-one" }
            div { class: "ambient ambient-two" }

            // ── Topbar (eyebrow + ghost overflow button) ─────────
            header { class: "topbar",
                p { class: "eyebrow", "VC Inventory" }
            }

            // ── Hero (trust row + H1 + subtitle + token graphic) ─
            section { class: "hero",
                div {
                    div { class: "trust-row",
                        span { class: "status-dot" }
                        span { "Verified credential" }
                    }
                    h1 { "Digital Passport" }
                    p { class: "subtitle",
                        "Passport-grade identity proof with selective "
                        "disclosure and predicate-only claims."
                    }
                }
                div { class: "credential-token",
                    span { class: "token-glow" }
                    span { class: "token-line" }
                    span { class: "token-line short" }
                }
            }

            // ── Identity strip (schema chip + holder-binding chip)
            div { class: "identity-strip",
                span { class: "version-chip", "{SCHEMA_ID}" }
                span { class: "binding-chip", "Holder bound" }
            }

            // ── Meta grid (4 columns, wraps on narrow viewports)
            section { class: "meta-grid",
                article { class: "meta-item",
                    span { "Issuer" }
                    strong { title: "{vc.issuer_did}", "{issuer_short}" }
                }
                article { class: "meta-item",
                    span { "Credential ID" }
                    strong { title: "{vc.vc_uri}", "{uri_short}" }
                }
                article { class: "meta-item",
                    span { "Issued" }
                    strong { "{issued_iso}" }
                }
                article { class: "meta-item success",
                    span { "Holder binding" }
                    strong { "Explicit proof" }
                }
            }

            // ── Claims header + primary CTA ──────────────────────
            section { class: "claims-header",
                div {
                    h2 { "Available proofs" }
                    p { "Choose exactly what to reveal to a verifier." }
                }
                // Generate VP lands in Phase 2 — disabled for now
                // so the button still anchors the card visually but
                // can't fire a half-implemented flow.
                button {
                    class: "primary-button",
                    disabled: true,
                    title: "Wired in Phase 2",
                    "Generate VP"
                }
            }

            // ── Claims list ──────────────────────────────────────
            section { class: "claims-list",

                // firstName — selective disclosure
                article { class: "claim-card",
                    div { class: "claim-copy",
                        span { class: "claim-type", "Selective disclosure" }
                        h3 { "First name" }
                        p {
                            if first_name_present {
                                if reveal_first {
                                    "{first_name_revealed}"
                                } else {
                                    "Encrypted until you reveal it."
                                }
                            } else {
                                "No opening stored — cannot reveal."
                            }
                        }
                    }
                    if first_name_present {
                        button {
                            class: "claim-action",
                            onclick: move |_| on_toggle_first.call(()),
                            {if reveal_first { "Hide" } else { "Reveal" }}
                        }
                    }
                }

                // lastName — selective disclosure
                article { class: "claim-card",
                    div { class: "claim-copy",
                        span { class: "claim-type", "Selective disclosure" }
                        h3 { "Last name" }
                        p {
                            if last_name_present {
                                if reveal_last {
                                    "{last_name_revealed}"
                                } else {
                                    "Encrypted until you reveal it."
                                }
                            } else {
                                "No opening stored — cannot reveal."
                            }
                        }
                    }
                    if last_name_present {
                        button {
                            class: "claim-action",
                            onclick: move |_| on_toggle_last.call(()),
                            {if reveal_last { "Hide" } else { "Reveal" }}
                        }
                    }
                }

                // dateOfBirth — predicate-only
                article { class: "claim-card predicate",
                    div { class: "claim-copy",
                        span { class: "claim-type purple", "Predicate-only" }
                        h3 { "Date of birth" }
                        p {
                            "Never reveals the raw date. Proves only "
                            "age ≥ threshold."
                            if !dob_present {
                                " (no opening stored — predicate cannot prove)"
                            }
                        }
                    }
                    div { class: "threshold-control",
                        span { "Age over" }
                        input {
                            r#type: "number",
                            inputmode: "numeric",
                            min: "1",
                            max: "120",
                            value: "{age_threshold}",
                            onchange: move |evt| {
                                if let Ok(n) = evt.value().parse::<u32>() {
                                    on_threshold_change.call(n);
                                }
                            },
                        }
                        span { "years" }
                    }
                }
            }

            // ── Footer (delete + verify badge + Phase 2 hint) ───
            footer { class: "footer-actions",
                {
                    let vc_uri = vc.vc_uri.clone();
                    rsx! {
                        if let Some(handler) = on_delete {
                            button {
                                class: "danger-button",
                                title: "Remove this credential from the wallet (local-only — no chain op)",
                                onclick: move |_| handler.call(vc_uri.clone()),
                                "Delete"
                            }
                        }
                    }
                }
                p {
                    if let Some(badge) = verify_label.as_ref() {
                        "{badge} · "
                    }
                    "Presentation generation is wired in Phase 2."
                }
            }
        }
    }
}

/// Mid-truncate a string to `max` chars total (head + "…" + tail).
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.into();
    }
    let head: String = s.chars().take(max / 2).collect();
    let tail_start = s.chars().count().saturating_sub(max / 2);
    let tail: String = s.chars().skip(tail_start).collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_text_padded_strips_nuls() {
        let mut buf = vec![0u8; 64];
        buf[..5].copy_from_slice(b"Alice");
        assert_eq!(decode_text_padded(&buf), "Alice");
    }

    #[test]
    fn decode_text_padded_handles_empty() {
        let buf = vec![0u8; 64];
        assert_eq!(decode_text_padded(&buf), "");
    }

    #[test]
    fn decode_days_since_epoch_matches_iso() {
        // 1990-01-01 = day 7305 since 1970-01-01
        let buf = 7305u32.to_le_bytes().to_vec();
        assert_eq!(decode_days_since_epoch(&buf), "1990-01-01");
    }

    #[test]
    fn decode_days_since_epoch_wrong_len_falls_back() {
        let buf = vec![0u8, 1, 2];
        assert_eq!(decode_days_since_epoch(&buf), "<3 bytes>");
    }

    #[test]
    fn matcher_accepts_prefixed_uri() {
        let vc = StoredVc {
            vc_uri: "urn:vc:digital-passport:abc".into(),
            issuer_did: "did:midnight:issuer".into(),
            holder_did: "did:midnight:holder".into(),
            format: "midnight-vc-compact".into(),
            body: vec![],
            issued_at_ms: 0,
        };
        assert!(is_digital_passport(&vc));
    }

    #[test]
    fn matcher_rejects_unrelated() {
        let vc = StoredVc {
            vc_uri: "urn:vc:birth-cert:abc".into(),
            issuer_did: "did:midnight:issuer".into(),
            holder_did: "did:midnight:holder".into(),
            format: "midnight-vc-compact".into(),
            body: vec![],
            issued_at_ms: 0,
        };
        assert!(!is_digital_passport(&vc));
    }

    #[test]
    fn truncate_short_strings_unchanged() {
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn truncate_long_strings_mid_elides() {
        let out = truncate("abcdefghijklmnop", 8);
        // 4 head chars + "…" + 4 tail chars
        assert_eq!(out, "abcd…mnop");
    }
}
