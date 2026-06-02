//! Authorization response — wallet → issuer body for the
//! `direct_post` response mode.
//!
//! Phase 1 only emits `id_token` + `state`; the struct holds
//! `vp_token` + `presentation_submission` as `Option<…>` with
//! `skip_serializing_if = "Option::is_none"` so Phase 2 builders
//! fill them without changing the type AND without polluting
//! the Phase-1 wire with `null` fields the issuer doesn't read.
//!
//! Reference: `docs/superpowers/specs/2026-06-02-login-with-did-architecture.md`.

use serde::{Deserialize, Serialize};

use crate::http::{HttpClient, HttpError};

/// Normative OID4VP / SIOPv2 authorization response body. Each
/// optional field is omitted on the wire when `None`.
#[derive(Debug, Clone, Serialize, Default)]
pub struct AuthorizationResponse {
    /// SIOPv2 self-issued id_token. Populated by the Phase-1
    /// `IdTokenBuilder`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,

    /// Verifiable Presentation token. Populated by the Phase-2
    /// `VpTokenBuilder`. Kept as `serde_json::Value` because
    /// the on-the-wire shape varies per format (`jwt_vp_json`,
    /// `sd-jwt-vc`, `ldp_vp`, `mdoc`), and a typed model would
    /// pre-commit to one. The Phase-2 builder will emit the
    /// format-appropriate JSON directly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vp_token: Option<serde_json::Value>,

    /// Presentation Submission (DIF PEX 2.0) mapping
    /// `input_descriptors` → submitted credentials. Phase 2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation_submission: Option<serde_json::Value>,

    /// State value echoed back to the issuer for session
    /// correlation. Populated from `AuthorizationRequest::state`
    /// at coordinator entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

impl AuthorizationResponse {
    /// Empty response carrying only the echoed state. Builders
    /// mutate it in registered order.
    pub fn new(state: Option<String>) -> Self {
        Self {
            state,
            ..Default::default()
        }
    }
}

/// Issuer's response to the wallet's POST. Mirrors what the
/// IssuerDIDIT-mock /authorize-response route returns (plus a
/// `redirect_to` field the wallet ignores — that's for the
/// laptop browser's polling script).
#[derive(Debug, Clone, Deserialize)]
pub struct PostResponseResult {
    pub session_id: String,
    pub status: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PostResponseError {
    #[error("http error: {0}")]
    Http(String),
    #[error("non-2xx status {status}: {body}")]
    Status { status: u16, body: String },
    #[error("decode error: {0}")]
    Decode(#[from] serde_json::Error),
}

impl From<HttpError> for PostResponseError {
    fn from(e: HttpError) -> Self {
        PostResponseError::Http(e.to_string())
    }
}

/// POST the authorization response to the issuer's
/// `response_uri`. Body is `Content-Type: application/json`; no
/// bearer auth (the SIOPv2 id_token IS the auth).
pub async fn post_response(
    http: &dyn HttpClient,
    response_uri: &str,
    resp: &AuthorizationResponse,
) -> Result<PostResponseResult, PostResponseError> {
    let body = serde_json::to_value(resp)?;
    let r = http.post_json(response_uri, &body, None).await?;
    let body_text = r
        .body_text()
        .map_err(|e| PostResponseError::Http(e.to_string()))?
        .to_string();
    if !r.is_success() {
        return Err(PostResponseError::Status {
            status: r.status,
            body: body_text,
        });
    }
    let parsed: PostResponseResult = serde_json::from_str(&body_text)?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::mock::MockHttpClient;
    use serde_json::{json, Value};

    #[test]
    fn empty_response_serializes_to_just_state() {
        let r = AuthorizationResponse::new(Some("st-1".into()));
        let v = serde_json::to_value(&r).unwrap();
        // Only `state` is on the wire — Phase-1 hygiene.
        assert_eq!(v, json!({ "state": "st-1" }));
    }

    #[test]
    fn id_token_only_response_omits_vp_fields() {
        let r = AuthorizationResponse {
            id_token: Some("abc.def.ghi".into()),
            state: Some("st-1".into()),
            ..Default::default()
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(
            v,
            json!({
                "id_token": "abc.def.ghi",
                "state": "st-1",
            })
        );
        // Defensive: no `vp_token: null` / `presentation_submission: null`.
        let map = v.as_object().unwrap();
        assert!(!map.contains_key("vp_token"));
        assert!(!map.contains_key("presentation_submission"));
    }

    #[tokio::test]
    async fn post_response_returns_session_and_status() {
        let http = MockHttpClient::default();
        http.push_json(
            200,
            &json!({ "session_id": "S-1", "status": "authenticated" }),
        );
        let resp = AuthorizationResponse {
            id_token: Some("abc.def.ghi".into()),
            state: Some("st-1".into()),
            ..Default::default()
        };
        let r = post_response(
            &http,
            "https://issuer.local/authorize-response",
            &resp,
        )
        .await
        .expect("ok");
        assert_eq!(r.session_id, "S-1");
        assert_eq!(r.status, "authenticated");

        let rec = http.recorded();
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].method, "POST");
        assert_eq!(rec[0].url, "https://issuer.local/authorize-response");
        let body: &Value = rec[0].body.as_ref().expect("body recorded");
        assert_eq!(body["id_token"], "abc.def.ghi");
        assert_eq!(body["state"], "st-1");
        // `vp_token` / `presentation_submission` not present.
        assert!(body.as_object().unwrap().get("vp_token").is_none());
        assert!(rec[0].bearer.is_none());
    }

    #[tokio::test]
    async fn post_response_surfaces_non_2xx_with_body() {
        let http = MockHttpClient::default();
        http.push_status_body(401, b"nonce mismatch");
        let resp = AuthorizationResponse::new(None);
        let err = post_response(&http, "https://issuer.local/x", &resp)
            .await
            .expect_err("must fail");
        match err {
            PostResponseError::Status { status: 401, body } => {
                assert!(body.contains("nonce"));
            }
            other => panic!("expected 401 Status, got {other:?}"),
        }
    }
}
