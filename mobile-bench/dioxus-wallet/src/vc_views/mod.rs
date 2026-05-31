//! Schema-specific Verifiable Credential view components.
//!
//! The Identity Centre's VC inventory uses a generic fallback row
//! (truncated `vc_uri` + Self-verify button) for every VC. When a
//! VC matches a known schema, the inventory dispatches to a card
//! component in this module instead — making privacy tiers
//! (hidden / selectively disclosable / predicate-only) visually
//! obvious to a demo audience.
//!
//! # Extraction plan
//!
//! Each submodule takes narrow inputs (`StoredVc`, opening fetcher
//! closure, event callbacks) with **no `BridgeState` / `Network`
//! coupling** so that the component can be moved verbatim into the
//! upstream `midnight-verifiable-credentials` repo alongside its
//! credential family. The dispatch site in `identity_centre.rs`
//! owns the redb plumbing; the view stays pure.
//!
//! When extracting, take:
//! - `digital_passport.rs` → `packages/prototypes/credential-families/digital-passport/views/dioxus/`
//! - the matching `.vc-card-passport` block from `assets/styles.css`
//!
//! and re-import from that location.

pub mod digital_passport;
