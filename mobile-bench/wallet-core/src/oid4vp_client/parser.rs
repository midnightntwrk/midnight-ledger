//! OID4VP / SIOPv2 request URL parser + request-object fetcher.
//!
//! Phase 1 only consumes the "pure authentication" subset of the
//! protocol: the request carries no `presentation_definition`, so
//! [`AuthRequest`] only models the fields the wallet actually needs
//! to mint a SIOPv2 id_token and POST it back.

use serde::{Deserialize, Serialize};
use url::Url;

/// Parsed SIOPv2 authorization request — the subset Phase 1 cares about.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRequest {
    pub client_id: String,
    pub nonce: String,
    pub state: Option<String>,
    /// Server-supplied URI to POST `{id_token, state}` back to.
    /// Phase 1 expects this on the top level of the request object;
    /// real OID4VP allows it inside the request JWS but we keep it
    /// simple.
    pub redirect_uri: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Oid4vpParseError {
    #[error("not an openid4vp:// URL: {0}")]
    BadScheme(String),
    #[error("missing required query param: {0}")]
    MissingParam(&'static str),
    #[error("url parse error: {0}")]
    Url(#[from] url::ParseError),
    #[error("http error fetching request_uri: {0}")]
    Http(String),
    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Extract `request_uri` from an `openid4vp://...` URL.
pub fn parse_request_url(url: &str) -> Result<String, Oid4vpParseError> {
    let u = Url::parse(url)?;
    if u.scheme() != "openid4vp" {
        return Err(Oid4vpParseError::BadScheme(u.scheme().into()));
    }
    let request_uri = u
        .query_pairs()
        .find(|(k, _)| k == "request_uri")
        .map(|(_, v)| v.into_owned())
        .ok_or(Oid4vpParseError::MissingParam("request_uri"))?;
    Ok(request_uri)
}

/// GET the request object from `request_uri` and parse it.
pub async fn fetch_request_object(request_uri: &str) -> Result<AuthRequest, Oid4vpParseError> {
    let body = reqwest::get(request_uri)
        .await
        .map_err(|e| Oid4vpParseError::Http(e.to_string()))?
        .error_for_status()
        .map_err(|e| Oid4vpParseError::Http(e.to_string()))?
        .text()
        .await
        .map_err(|e| Oid4vpParseError::Http(e.to_string()))?;
    let req: AuthRequest = serde_json::from_str(&body)?;
    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_extracts_request_uri() {
        let url = "openid4vp://issuer.local/?request_uri=https%3A%2F%2Fissuer.local%2Frequest%2Fabc";
        let r = parse_request_url(url).expect("ok");
        assert_eq!(r, "https://issuer.local/request/abc");
    }

    #[test]
    fn parse_url_rejects_wrong_scheme() {
        let err = parse_request_url("https://issuer/?request_uri=x").expect_err("bad");
        assert!(matches!(err, Oid4vpParseError::BadScheme(_)));
    }

    #[test]
    fn parse_url_requires_request_uri_param() {
        let err = parse_request_url("openid4vp://issuer.local/").expect_err("missing");
        assert!(matches!(err, Oid4vpParseError::MissingParam("request_uri")));
    }
}
