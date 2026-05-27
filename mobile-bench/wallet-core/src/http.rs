//! HTTP-client port for the OID4VP / OID4VCI flows. The real-deps
//! adapter (`ReqwestHttpClient`) lives next to the trait; a
//! `MockHttpClient` lives behind `#[cfg(any(test, feature =
//! "test-support"))]` for unit tests.
//!
//! Same dependency-injection shape as `IndexerClient`,
//! `NodeClient`, `Prover` (see `chain.rs`): consumers take an
//! `&dyn HttpClient` (or `Arc<dyn HttpClient>`) and never touch
//! `reqwest` directly.

use async_trait::async_trait;

/// Wire-level HTTP response.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
    pub fn body_text(&self) -> Result<&str, HttpError> {
        std::str::from_utf8(&self.body)
            .map_err(|e| HttpError::Codec(format!("utf-8: {e}")))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("http transport: {0}")]
    Transport(String),
    #[error("http codec: {0}")]
    Codec(String),
}

#[async_trait]
pub trait HttpClient: Send + Sync + 'static {
    /// GET. Always returns the response — non-2xx is NOT an error
    /// here; callers decide how to handle status codes.
    async fn get(&self, url: &str) -> Result<HttpResponse, HttpError>;

    /// POST with a JSON body. Content-Type set to application/json.
    async fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
        bearer: Option<&str>,
    ) -> Result<HttpResponse, HttpError>;
}

/// Real-deps adapter using `reqwest`. Stateless; share an Arc.
#[derive(Debug, Default)]
pub struct ReqwestHttpClient;

#[async_trait]
impl HttpClient for ReqwestHttpClient {
    async fn get(&self, url: &str) -> Result<HttpResponse, HttpError> {
        let resp = reqwest::Client::new()
            .get(url)
            .send()
            .await
            .map_err(|e| HttpError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let body = resp
            .bytes()
            .await
            .map_err(|e| HttpError::Transport(e.to_string()))?;
        Ok(HttpResponse { status, body: body.to_vec() })
    }

    async fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
        bearer: Option<&str>,
    ) -> Result<HttpResponse, HttpError> {
        let mut req = reqwest::Client::new().post(url).json(body);
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| HttpError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let body = resp
            .bytes()
            .await
            .map_err(|e| HttpError::Transport(e.to_string()))?;
        Ok(HttpResponse { status, body: body.to_vec() })
    }
}

/// Test adapter: scripted responses + recorded requests. Behind
/// the `test-support` feature so production builds don't carry it.
#[cfg(any(test, feature = "test-support"))]
pub mod mock {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[derive(Debug, Clone)]
    pub struct RecordedRequest {
        pub method: &'static str,
        pub url: String,
        pub body: Option<serde_json::Value>,
        pub bearer: Option<String>,
    }

    #[derive(Default)]
    pub struct MockHttpClient {
        responses: Mutex<VecDeque<Result<HttpResponse, HttpError>>>,
        requests: Mutex<Vec<RecordedRequest>>,
    }

    impl MockHttpClient {
        pub fn push_response(&self, resp: Result<HttpResponse, HttpError>) {
            self.responses.lock().unwrap().push_back(resp);
        }
        pub fn push_json(&self, status: u16, body: &serde_json::Value) {
            let bytes = serde_json::to_vec(body).expect("encode mock body");
            self.push_response(Ok(HttpResponse { status, body: bytes }));
        }
        pub fn push_status_body(&self, status: u16, body: &[u8]) {
            self.push_response(Ok(HttpResponse {
                status,
                body: body.to_vec(),
            }));
        }
        pub fn recorded(&self) -> Vec<RecordedRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl HttpClient for MockHttpClient {
        async fn get(&self, url: &str) -> Result<HttpResponse, HttpError> {
            self.requests.lock().unwrap().push(RecordedRequest {
                method: "GET",
                url: url.to_string(),
                body: None,
                bearer: None,
            });
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| {
                    Err(HttpError::Transport("MockHttpClient: no response queued".into()))
                })
        }
        async fn post_json(
            &self,
            url: &str,
            body: &serde_json::Value,
            bearer: Option<&str>,
        ) -> Result<HttpResponse, HttpError> {
            self.requests.lock().unwrap().push(RecordedRequest {
                method: "POST",
                url: url.to_string(),
                body: Some(body.clone()),
                bearer: bearer.map(str::to_string),
            });
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| {
                    Err(HttpError::Transport("MockHttpClient: no response queued".into()))
                })
        }
    }
}
