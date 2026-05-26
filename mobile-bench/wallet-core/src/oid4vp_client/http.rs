//! Final leg of the OID4VP flow — POST the signed id_token to
//! the issuer's redirect_uri and read back the session_id +
//! status.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct AuthResponseBody<'a> {
    id_token: &'a str,
    state: Option<&'a str>,
}

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

pub async fn post_response(
    redirect_uri: &str,
    id_token: &str,
    state: Option<&str>,
) -> Result<PostResponseResult, PostResponseError> {
    let body = AuthResponseBody { id_token, state };
    let resp = reqwest::Client::new()
        .post(redirect_uri)
        .json(&body)
        .send()
        .await
        .map_err(|e| PostResponseError::Http(e.to_string()))?;
    let status = resp.status();
    let body_text = resp
        .text()
        .await
        .map_err(|e| PostResponseError::Http(e.to_string()))?;
    if !status.is_success() {
        return Err(PostResponseError::Status {
            status: status.as_u16(),
            body: body_text,
        });
    }
    let parsed: PostResponseResult = serde_json::from_str(&body_text)?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{body_partial_json, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    async fn post_response_returns_session_and_status() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/authorize-response"))
            .and(body_partial_json(serde_json::json!({ "id_token": "abc.def.ghi" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "session_id": "S-1",
                "status": "authenticated"
            })))
            .mount(&mock)
            .await;

        let url = format!("{}/authorize-response", mock.uri());
        let r = post_response(&url, "abc.def.ghi", Some("st-1"))
            .await
            .expect("ok");
        assert_eq!(r.session_id, "S-1");
        assert_eq!(r.status, "authenticated");
    }

    #[tokio::test]
    async fn post_response_reports_4xx_specifically() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string("nonce mismatch"))
            .mount(&mock)
            .await;
        let err = post_response(&format!("{}/x", mock.uri()), "j", None)
            .await
            .expect_err("must fail");
        match err {
            PostResponseError::Status { status: 401, body } => assert!(body.contains("nonce")),
            other => panic!("expected 401 Status, got {other:?}"),
        }
    }
}
