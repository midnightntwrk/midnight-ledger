//! Authorization request — the issuer-sent object the wallet
//! parses out of `request_uri`.
//!
//! Phase 1 only fills the `id_token`-relevant fields; the struct
//! shape mirrors the full normative OID4VP request so adding
//! `presentation_definition` + Mode-B handling later is a
//! field-level extension, not a type replacement.
//!
//! ## Transitional dual-read
//!
//! Issuer-mock pre-Task-7 still sends `redirect_uri` (the legacy
//! SIOPv1 field name). The normative shape uses `response_uri`
//! for `direct_post` mode. Until both ends are upgraded (Tasks 7
//! and 8), the wallet accepts whichever is present and maps it
//! to `response_uri`. Task 9 drops the legacy fallback once both
//! sides emit the normative field.
//!
//! Reference: see the Login-with-DID architecture spec at
//! `docs/superpowers/specs/2026-06-02-login-with-did-architecture.md`.

use serde::{Deserialize, Serialize};
use url::Url;

use crate::http::{HttpClient, HttpError};

/// OID4VP / SIOPv2 response_type. Phase 1 only accepts
/// `id_token`; the other arms exist on the wire surface so the
/// parser rejects them with a clear, typed error rather than a
/// generic decode failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseType {
    /// SIOPv2 self-issued id_token (Mode A) — Phase 1.
    #[serde(rename = "id_token")]
    IdToken,
    /// Mode B (id_token + vp_token).
    #[serde(rename = "vp_token id_token")]
    VpTokenIdToken,
    /// Mode C (vp_token only).
    #[serde(rename = "vp_token")]
    VpToken,
}

/// OID4VP response_mode. Phase 1 only uses `direct_post`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseMode {
    /// Wallet POSTs the response as `application/json` to
    /// `response_uri`. Used by every Phase-1 flow.
    DirectPost,
    /// Wallet wraps the response in a JWS before POSTing.
    /// Phase 3 (not on this branch).
    #[serde(rename = "direct_post.jwt")]
    DirectPostJwt,
}

/// Phase-1 placeholder. The normative `PresentationDefinition`
/// (DIF PEX 2.0) has a non-trivial shape; we keep it as
/// `serde_json::Value` for now so the wire surface accepts
/// future requests without a schema rev. Phase 2 swaps in a
/// typed model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresentationDefinition {
    #[serde(flatten)]
    pub raw: serde_json::Value,
}

/// Normative OID4VP / SIOPv2 authorization request.
///
/// Phase 1 reads `client_id`, `nonce`, `state`, `response_uri`,
/// and rejects anything where `response_type != IdToken`. The
/// remaining fields are parsed but not yet acted on; later phases
/// add behaviour without changing this shape.
#[derive(Debug, Clone)]
pub struct AuthorizationRequest {
    pub client_id: String,
    pub response_type: ResponseType,
    pub response_mode: ResponseMode,
    /// `direct_post` POST target. Maps from either the normative
    /// `response_uri` field or the legacy `redirect_uri` (during
    /// the issuer-rewrite transition — see file-level note).
    pub response_uri: String,
    pub scope: String,
    pub nonce: String,
    /// Per OID4VP §"Authorization Request Parameters", `state` is
    /// optional. Issuer-mock Phase-1 always populates it (it's
    /// the session id), but the type permits its absence so we
    /// can talk to verifiers that don't echo it.
    pub state: Option<String>,
    /// Phase 2.
    pub presentation_definition: Option<PresentationDefinition>,
}

/// Wire-level deserialization helper. Holds every field that
/// might appear in either Phase-1 mock-issuer output or a
/// production-shape request object, including the legacy
/// `redirect_uri`. The `From` impl below collapses the two paths
/// into one canonical [`AuthorizationRequest`].
#[derive(Debug, Deserialize)]
struct RawRequest {
    client_id: String,
    /// Optional only because the legacy issuer-mock didn't emit
    /// it. Defaults to `id_token` on parse so existing demo
    /// traffic keeps flowing during the transition.
    response_type: Option<ResponseType>,
    response_mode: Option<ResponseMode>,
    /// Normative target for `direct_post` mode.
    response_uri: Option<String>,
    /// Legacy SIOPv1 / pre-rewrite issuer-mock field. Mapped to
    /// `response_uri` if `response_uri` is absent. Removed in
    /// Task 9.
    redirect_uri: Option<String>,
    scope: Option<String>,
    nonce: String,
    state: Option<String>,
    presentation_definition: Option<PresentationDefinition>,
}

impl From<RawRequest> for AuthorizationRequest {
    fn from(r: RawRequest) -> Self {
        Self {
            client_id: r.client_id,
            response_type: r.response_type.unwrap_or(ResponseType::IdToken),
            response_mode: r.response_mode.unwrap_or(ResponseMode::DirectPost),
            response_uri: r.response_uri.or(r.redirect_uri).unwrap_or_default(),
            // Per OID4VP §"Authorization Request Parameters",
            // `scope` defaults to `openid` for SIOPv2 flows.
            scope: r.scope.unwrap_or_else(|| "openid".to_string()),
            nonce: r.nonce,
            state: r.state,
            presentation_definition: r.presentation_definition,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RequestParseError {
    #[error("not an openid4vp:// URL: scheme={0}")]
    BadScheme(String),
    #[error("missing required query param: {0}")]
    MissingParam(&'static str),
    #[error("url parse error: {0}")]
    Url(#[from] url::ParseError),
    #[error("http error fetching request_uri: {0}")]
    Http(String),
    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),
    /// `response_uri` was empty AND `redirect_uri` was absent —
    /// neither the normative nor the legacy path produced a
    /// target. Most likely a mis-encoded request object.
    #[error("request object missing response_uri / redirect_uri")]
    MissingResponseTarget,
    /// Wallet got handed a request asking for `vp_token` (Mode
    /// B/C). Phase 1 only supports `id_token`. Surface as a
    /// distinct variant so the UI can show a "future feature"
    /// affordance instead of a generic parse error.
    #[error(
        "unsupported response_type {0:?}; this build only handles \
         id_token (Phase 1)"
    )]
    UnsupportedMode(ResponseType),
}

impl From<HttpError> for RequestParseError {
    fn from(e: HttpError) -> Self {
        RequestParseError::Http(e.to_string())
    }
}

/// Extract `request_uri` from an `openid4vp://...` URL. The
/// `request_uri` is the issuer's URL where the signed request
/// object lives; the wallet GETs it next.
pub fn parse_request_url(url: &str) -> Result<String, RequestParseError> {
    let u = Url::parse(url)?;
    if u.scheme() != "openid4vp" {
        return Err(RequestParseError::BadScheme(u.scheme().to_string()));
    }
    u.query_pairs()
        .find(|(k, _)| k == "request_uri")
        .map(|(_, v)| v.into_owned())
        .ok_or(RequestParseError::MissingParam("request_uri"))
}

/// GET the request object from `request_uri` and parse it into a
/// typed [`AuthorizationRequest`]. Rejects Mode B/C with
/// [`RequestParseError::UnsupportedMode`] until later phases
/// land vp_token support.
pub async fn fetch_request_object(
    http: &dyn HttpClient,
    request_uri: &str,
) -> Result<AuthorizationRequest, RequestParseError> {
    let resp = http.get(request_uri).await?;
    if !resp.is_success() {
        return Err(RequestParseError::Http(format!(
            "non-2xx status {} fetching request_uri",
            resp.status
        )));
    }
    let body = resp.body_text()?;
    let raw: RawRequest = serde_json::from_str(body)?;
    let req: AuthorizationRequest = raw.into();
    // Defensive: if both `response_uri` and `redirect_uri` were
    // absent, `unwrap_or_default()` produced an empty string. A
    // wallet that POSTs to "" would surface a confusing transport
    // error later; fail fast with a typed reason here.
    if req.response_uri.is_empty() {
        return Err(RequestParseError::MissingResponseTarget);
    }
    if req.response_type != ResponseType::IdToken {
        return Err(RequestParseError::UnsupportedMode(req.response_type));
    }
    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::mock::MockHttpClient;
    use serde_json::json;

    #[test]
    fn parse_url_extracts_request_uri() {
        let url = "openid4vp://issuer.local/?request_uri=https%3A%2F%2Fissuer.local%2Frequest%2Fabc";
        let r = parse_request_url(url).expect("ok");
        assert_eq!(r, "https://issuer.local/request/abc");
    }

    #[test]
    fn parse_url_rejects_wrong_scheme() {
        let err = parse_request_url("https://issuer/?request_uri=x").expect_err("bad");
        assert!(matches!(err, RequestParseError::BadScheme(_)));
    }

    #[test]
    fn parse_url_requires_request_uri_param() {
        let err = parse_request_url("openid4vp://issuer.local/").expect_err("missing");
        assert!(matches!(err, RequestParseError::MissingParam("request_uri")));
    }

    #[tokio::test]
    async fn fetch_request_object_parses_normative_shape() {
        let http = MockHttpClient::default();
        http.push_json(
            200,
            &json!({
                "client_id": "demo-issuer",
                "response_type": "id_token",
                "response_mode": "direct_post",
                "response_uri": "https://issuer.local/authorize-response",
                "scope": "openid",
                "nonce": "nonce-x",
                "state": "st-x",
            }),
        );
        let req = fetch_request_object(&http, "https://issuer.local/request/abc")
            .await
            .expect("ok");
        assert_eq!(req.client_id, "demo-issuer");
        assert_eq!(req.response_type, ResponseType::IdToken);
        assert_eq!(req.response_mode, ResponseMode::DirectPost);
        assert_eq!(req.response_uri, "https://issuer.local/authorize-response");
        assert_eq!(req.scope, "openid");
        assert_eq!(req.nonce, "nonce-x");
        assert_eq!(req.state.as_deref(), Some("st-x"));
        assert!(req.presentation_definition.is_none());
    }

    #[tokio::test]
    async fn fetch_request_object_dual_reads_redirect_uri() {
        // Legacy issuer-mock body: `redirect_uri` instead of
        // `response_uri`, no `response_type`, no `response_mode`,
        // no `scope`. Parser must accept it during the rewrite
        // transition.
        let http = MockHttpClient::default();
        http.push_json(
            200,
            &json!({
                "client_id": "demo-issuer",
                "nonce": "nonce-x",
                "state": "st-x",
                "redirect_uri": "https://issuer.local/authorize-response",
            }),
        );
        let req = fetch_request_object(&http, "https://issuer.local/request/abc")
            .await
            .expect("ok");
        assert_eq!(req.response_uri, "https://issuer.local/authorize-response");
        assert_eq!(req.response_type, ResponseType::IdToken);
        assert_eq!(req.response_mode, ResponseMode::DirectPost);
        // Default-filled.
        assert_eq!(req.scope, "openid");
    }

    #[tokio::test]
    async fn fetch_request_object_rejects_vp_token_mode() {
        let http = MockHttpClient::default();
        http.push_json(
            200,
            &json!({
                "client_id": "demo-issuer",
                "response_type": "vp_token id_token",
                "response_uri": "https://issuer.local/authorize-response",
                "nonce": "nonce-x",
            }),
        );
        let err = fetch_request_object(&http, "https://issuer.local/request/abc")
            .await
            .expect_err("must fail");
        assert!(matches!(
            err,
            RequestParseError::UnsupportedMode(ResponseType::VpTokenIdToken)
        ));
    }

    #[tokio::test]
    async fn fetch_request_object_rejects_missing_response_target() {
        let http = MockHttpClient::default();
        http.push_json(
            200,
            &json!({
                "client_id": "demo-issuer",
                "nonce": "nonce-x",
            }),
        );
        let err = fetch_request_object(&http, "https://issuer.local/request/abc")
            .await
            .expect_err("must fail");
        assert!(matches!(err, RequestParseError::MissingResponseTarget));
    }

    #[tokio::test]
    async fn fetch_request_object_rejects_non_2xx() {
        let http = MockHttpClient::default();
        http.push_status_body(500, b"oops");
        let err = fetch_request_object(&http, "https://issuer.local/request/abc")
            .await
            .expect_err("must fail");
        assert!(matches!(err, RequestParseError::Http(_)));
    }
}
