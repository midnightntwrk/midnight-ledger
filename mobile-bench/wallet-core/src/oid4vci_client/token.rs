//! POST `/token` with the pre-authorized code, return the access
//! token + c_nonce. The c_nonce becomes the JWS nonce in the
//! subsequent `/credential` proof.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct TokenRequest<'a> {
    grant_type: &'a str,
    #[serde(rename = "pre-authorized_code")]
    pre_authorized_code: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub c_nonce: String,
    pub token_type: String,
    pub expires_in: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum Oid4vciTokenError {
    #[error("http: {0}")]
    Http(String),
    #[error("non-2xx {status}: {body}")]
    Status { status: u16, body: String },
    #[error("decode: {0}")]
    Decode(#[from] serde_json::Error),
}

/// POST to `{issuer}/token` with the pre-authorized code,
/// return the access token + c_nonce.
pub async fn request_token(
    issuer: &str,
    pre_authorized_code: &str,
) -> Result<TokenResponse, Oid4vciTokenError> {
    let url = format!("{}/token", issuer.trim_end_matches('/'));
    let body = TokenRequest {
        grant_type: "urn:ietf:params:oauth:grant-type:pre-authorized_code",
        pre_authorized_code,
    };
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| Oid4vciTokenError::Http(e.to_string()))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| Oid4vciTokenError::Http(e.to_string()))?;
    if !status.is_success() {
        return Err(Oid4vciTokenError::Status {
            status: status.as_u16(),
            body: text,
        });
    }
    Ok(serde_json::from_str(&text)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{matchers::*, Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn request_token_round_trips() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_partial_json(serde_json::json!({
                "grant_type": "urn:ietf:params:oauth:grant-type:pre-authorized_code",
                "pre-authorized_code": "C1"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "AT-1",
                "c_nonce": "CN-1",
                "token_type": "Bearer",
                "expires_in": 600
            })))
            .mount(&mock)
            .await;
        let t = request_token(&mock.uri(), "C1").await.expect("ok");
        assert_eq!(t.access_token, "AT-1");
        assert_eq!(t.c_nonce, "CN-1");
    }

    #[tokio::test]
    async fn request_token_surfaces_400_with_body() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_string("invalid_grant"))
            .mount(&mock)
            .await;
        let err = request_token(&mock.uri(), "X").await.expect_err("err");
        match err {
            Oid4vciTokenError::Status { status: 400, body } => assert_eq!(body, "invalid_grant"),
            other => panic!("expected 400 Status, got {other:?}"),
        }
    }
}
