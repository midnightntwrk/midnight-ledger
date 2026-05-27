//! POST `/token` with the pre-authorized code, return the access
//! token + c_nonce. The c_nonce becomes the JWS nonce in the
//! subsequent `/credential` proof.

use serde::Deserialize;

use crate::http::{HttpClient, HttpError};

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

impl From<HttpError> for Oid4vciTokenError {
    fn from(e: HttpError) -> Self {
        Oid4vciTokenError::Http(e.to_string())
    }
}

/// POST to `{issuer}/token` with the pre-authorized code,
/// return the access token + c_nonce.
pub async fn request_token(
    http: &dyn HttpClient,
    issuer: &str,
    pre_authorized_code: &str,
) -> Result<TokenResponse, Oid4vciTokenError> {
    let url = format!("{}/token", issuer.trim_end_matches('/'));
    let body = serde_json::json!({
        "grant_type": "urn:ietf:params:oauth:grant-type:pre-authorized_code",
        "pre-authorized_code": pre_authorized_code,
    });
    let resp = http.post_json(&url, &body, None).await?;
    let text = resp
        .body_text()
        .map_err(|e| Oid4vciTokenError::Http(e.to_string()))?
        .to_string();
    if !resp.is_success() {
        return Err(Oid4vciTokenError::Status {
            status: resp.status,
            body: text,
        });
    }
    Ok(serde_json::from_str(&text)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::mock::MockHttpClient;
    use serde_json::json;

    #[tokio::test]
    async fn request_token_round_trips() {
        let http = MockHttpClient::default();
        http.push_json(
            200,
            &json!({
                "access_token": "AT-1",
                "c_nonce": "CN-1",
                "token_type": "Bearer",
                "expires_in": 600,
            }),
        );
        let t = request_token(&http, "https://issuer.local", "C1")
            .await
            .expect("ok");
        assert_eq!(t.access_token, "AT-1");
        assert_eq!(t.c_nonce, "CN-1");

        let rec = http.recorded();
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].method, "POST");
        assert_eq!(rec[0].url, "https://issuer.local/token");
        let body = rec[0].body.as_ref().expect("body recorded");
        assert_eq!(
            body["grant_type"],
            "urn:ietf:params:oauth:grant-type:pre-authorized_code"
        );
        assert_eq!(body["pre-authorized_code"], "C1");
    }

    #[tokio::test]
    async fn request_token_surfaces_400_with_body() {
        let http = MockHttpClient::default();
        http.push_status_body(400, b"invalid_grant");
        let err = request_token(&http, "https://issuer.local", "X")
            .await
            .expect_err("err");
        match err {
            Oid4vciTokenError::Status { status: 400, body } => assert_eq!(body, "invalid_grant"),
            other => panic!("expected 400 Status, got {other:?}"),
        }
    }
}
