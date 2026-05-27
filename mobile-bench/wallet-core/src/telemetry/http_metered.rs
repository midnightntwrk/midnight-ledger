//! Latency-recording decorator over any [`HttpClient`].
//!
//! Drop-in for production wiring:
//! ```ignore
//! let inner: Arc<dyn HttpClient> = Arc::new(ReqwestHttpClient::default());
//! let http = MeteredHttpClient::new(inner, metrics.clone());
//! oid4vci_run_issuance(&http, ...).await?;
//! ```
//!
//! Records `method`, host (from URL), status (or `err` on
//! transport failure), wall-time, and response-body length.
//! Status codes outside 200-299 still count as a successful
//! "got a response" — the decorator doesn't editorialise about
//! status semantics.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;

use super::{host_of, HttpRecord, Metrics};
use crate::http::{HttpClient, HttpError, HttpResponse};

pub struct MeteredHttpClient {
    inner: Arc<dyn HttpClient>,
    metrics: Arc<dyn Metrics>,
}

impl MeteredHttpClient {
    pub fn new(inner: Arc<dyn HttpClient>, metrics: Arc<dyn Metrics>) -> Self {
        Self { inner, metrics }
    }

    fn record(
        &self,
        method: &'static str,
        url: &str,
        started: Instant,
        result: &Result<HttpResponse, HttpError>,
    ) {
        let duration_ms = started.elapsed().as_millis() as u64;
        let host = host_of(url);
        match result {
            Ok(resp) => self.metrics.record_http(&HttpRecord {
                method,
                host,
                url,
                status: resp.status,
                duration_ms,
                body_bytes: Some(resp.body.len()),
                error: None,
            }),
            Err(e) => {
                let err_str = e.to_string();
                self.metrics.record_http(&HttpRecord {
                    method,
                    host,
                    url,
                    status: 0,
                    duration_ms,
                    body_bytes: None,
                    error: Some(&err_str),
                });
            }
        }
    }
}

#[async_trait]
impl HttpClient for MeteredHttpClient {
    async fn get(&self, url: &str) -> Result<HttpResponse, HttpError> {
        let started = Instant::now();
        let result = self.inner.get(url).await;
        self.record("GET", url, started, &result);
        result
    }
    async fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
        bearer: Option<&str>,
    ) -> Result<HttpResponse, HttpError> {
        let started = Instant::now();
        let result = self.inner.post_json(url, body, bearer).await;
        self.record("POST", url, started, &result);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::mock::MockHttpClient;
    use crate::telemetry::InMemoryMetrics;
    use serde_json::json;

    #[tokio::test]
    async fn records_ok_get() {
        let mock = Arc::new(MockHttpClient::default());
        mock.push_json(200, &json!({"hello": "world"}));
        let metrics: Arc<InMemoryMetrics> = Arc::new(InMemoryMetrics::new());
        let http = MeteredHttpClient::new(mock.clone() as Arc<dyn HttpClient>, metrics.clone());
        let resp = http.get("https://issuer.local/x").await.unwrap();
        assert_eq!(resp.status, 200);
        let snap = metrics.snapshot();
        let h = snap.http.get("GET issuer.local -> 200").expect("recorded");
        assert_eq!(h.count, 1);
        assert!(h.total_bytes > 0);
    }

    #[tokio::test]
    async fn records_post_with_status() {
        let mock = Arc::new(MockHttpClient::default());
        mock.push_json(201, &json!({"ok": true}));
        let metrics: Arc<InMemoryMetrics> = Arc::new(InMemoryMetrics::new());
        let http = MeteredHttpClient::new(mock.clone() as Arc<dyn HttpClient>, metrics.clone());
        http.post_json("https://issuer.local/token", &json!({}), Some("AT"))
            .await
            .unwrap();
        let snap = metrics.snapshot();
        assert!(snap.http.contains_key("POST issuer.local -> 201"));
        // Bearer + body still propagated to inner.
        let rec = mock.recorded();
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].bearer.as_deref(), Some("AT"));
    }

    #[tokio::test]
    async fn records_transport_error_in_err_bucket() {
        // Empty queue → MockHttpClient returns Transport error.
        let mock = Arc::new(MockHttpClient::default());
        let metrics: Arc<InMemoryMetrics> = Arc::new(InMemoryMetrics::new());
        let http = MeteredHttpClient::new(mock.clone() as Arc<dyn HttpClient>, metrics.clone());
        let res = http.get("https://issuer.local/missing").await;
        assert!(res.is_err());
        let snap = metrics.snapshot();
        let key = snap
            .http
            .keys()
            .find(|k| k.starts_with("GET issuer.local -> err"))
            .cloned();
        assert!(key.is_some(), "expected an err bucket, got {:?}", snap.http);
    }
}
