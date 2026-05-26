//! Client side of OID4VCI Pre-Authorized Code Flow for the
//! Midnight `birth` credential family.
//!
//! Steps:
//! 1. `offer::parse_offer_url` extracts the offer object from
//!    the QR's `openid-credential-offer://` URL.
//! 2. `token::request_token` exchanges the pre-auth code for an
//!    access token + c_nonce.
//! 3. `credential::request_credential` mints a DID-bound JWS
//!    proof over the c_nonce, POSTs `{proof, format}` to
//!    the credential endpoint, parses the VC + openings, and
//!    hands them to `vc_store` atomically.

mod offer;

pub use offer::{parse_offer_url, CredentialOffer, Grants, Oid4vciParseError, PreAuthorized};
