//! Client-side implementation of OID4VP / SIOPv2.
//!
//! Phase 1 only handles the "pure authentication" subset:
//! the request carries no presentation_definition; the wallet
//! responds with a signed id_token (no VP token). The flow:
//!
//! 1. User scans a QR carrying `openid4vp://...?request_uri=https://issuer/.../request/<id>`.
//! 2. `parser::parse_request_url` extracts the request_uri.
//! 3. `parser::fetch_request_object` GETs it, returning a typed AuthRequest.
//! 4. `jws::build_id_token` constructs the SIOPv2 id_token JWS.
//! 5. `http::post_response` POSTs `{id_token, state}` to redirect_uri.

mod jws;
mod parser;

pub use jws::{build_id_token, IdTokenError};
pub use parser::{parse_request_url, fetch_request_object, AuthRequest, Oid4vpParseError};
