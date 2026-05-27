//! Final leg of the OID4VP flow — POST the signed id_token to
//! the issuer's redirect_uri and read back the session_id +
//! status.

use serde::Deserialize;

use crate::http::{HttpClient, HttpError};

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

pub async fn post_response(
    http: &dyn HttpClient,
    redirect_uri: &str,
    id_token: &str,
    state: Option<&str>,
) -> Result<PostResponseResult, PostResponseError> {
    let body = serde_json::json!({
        "id_token": id_token,
        "state": state,
    });
    let resp = http.post_json(redirect_uri, &body, None).await?;
    let body_text = resp
        .body_text()
        .map_err(|e| PostResponseError::Http(e.to_string()))?
        .to_string();
    if !resp.is_success() {
        return Err(PostResponseError::Status {
            status: resp.status,
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
    use serde_json::json;

    #[tokio::test]
    async fn post_response_returns_session_and_status() {
        let http = MockHttpClient::default();
        http.push_json(
            200,
            &json!({"session_id": "S-1", "status": "authenticated"}),
        );
        let r = post_response(
            &http,
            "https://issuer.local/authorize-response",
            "abc.def.ghi",
            Some("st-1"),
        )
        .await
        .expect("ok");
        assert_eq!(r.session_id, "S-1");
        assert_eq!(r.status, "authenticated");

        let rec = http.recorded();
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].method, "POST");
        assert_eq!(rec[0].url, "https://issuer.local/authorize-response");
        let body = rec[0].body.as_ref().expect("body recorded");
        assert_eq!(body["id_token"], "abc.def.ghi");
        assert_eq!(body["state"], "st-1");
        assert!(rec[0].bearer.is_none());
    }

    #[tokio::test]
    async fn post_response_reports_4xx_specifically() {
        let http = MockHttpClient::default();
        http.push_status_body(401, b"nonce mismatch");
        let err = post_response(&http, "https://issuer.local/x", "j", None)
            .await
            .expect_err("must fail");
        match err {
            PostResponseError::Status { status: 401, body } => assert!(body.contains("nonce")),
            other => panic!("expected 401 Status, got {other:?}"),
        }
    }
}
