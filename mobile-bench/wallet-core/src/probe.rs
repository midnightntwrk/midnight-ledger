use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::crypto::ensure_default_crypto_provider;
use crate::network::Network;

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("reqwest: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("ws: {0}")]
    Ws(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("invalid url: {0}")]
    InvalidUrl(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeStatus {
    pub url: String,
    pub reachable: bool,
    pub latency_ms: u128,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeResult {
    pub network: Network,
    pub indexer_http: ProbeStatus,
    pub indexer_ws: ProbeStatus,
    pub node_ws: ProbeStatus,
}

impl ProbeResult {
    pub fn all_reachable(&self) -> bool {
        self.indexer_http.reachable && self.indexer_ws.reachable && self.node_ws.reachable
    }
}

/// Probe the indexer + node URLs for a given network. Each probe runs
/// with a 5-second budget; failures populate `detail` instead of
/// short-circuiting so the UI can show partial results.
pub async fn probe_connectivity(network: Network) -> ProbeResult {
    ensure_default_crypto_provider();
    let cfg = network.config();
    let timeout = Duration::from_secs(5);

    let (indexer_http, indexer_ws, node_ws) = tokio::join!(
        probe_indexer_http(cfg.indexer_http_url, timeout),
        // Indexer WS speaks graphql-transport-ws (per gsd-wallet's
        // wallet-sdk-facade); without the subprotocol header the server
        // returns HTTP 400 on the upgrade.
        probe_ws(cfg.indexer_ws_url, timeout, Some("graphql-transport-ws")),
        probe_ws(cfg.node_ws_url, timeout, None),
    );

    ProbeResult {
        network,
        indexer_http,
        indexer_ws,
        node_ws,
    }
}

/// `__typename` is part of every GraphQL spec-compliant schema and is
/// the cheapest possible reachability signal — no schema knowledge
/// required, no auth.
async fn probe_indexer_http(url: &str, timeout: Duration) -> ProbeStatus {
    let started = Instant::now();
    let result = async {
        let client = reqwest::Client::builder().timeout(timeout).build()?;
        let resp = client
            .post(url)
            .header("content-type", "application/json")
            .body(r#"{"query":"{ __typename }"}"#.to_string())
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Ok::<_, ProbeError>((status, body))
    }
    .await;

    let latency_ms = started.elapsed().as_millis();
    match result {
        Ok((status, body)) if status.is_success() => ProbeStatus {
            url: url.to_string(),
            reachable: true,
            latency_ms,
            detail: extract_typename(&body),
        },
        Ok((status, body)) => ProbeStatus {
            url: url.to_string(),
            reachable: false,
            latency_ms,
            detail: Some(format!("HTTP {status}: {}", truncate(&body, 120))),
        },
        Err(e) => ProbeStatus {
            url: url.to_string(),
            reachable: false,
            latency_ms,
            detail: Some(e.to_string()),
        },
    }
}

/// Open + immediately close a WebSocket. Sufficient to confirm the
/// host accepts a TLS handshake + upgrade; we do not exchange any
/// protocol frames yet. `subprotocol` populates `Sec-WebSocket-Protocol`
/// for endpoints that reject bare upgrades (e.g. graphql-transport-ws).
async fn probe_ws(url: &str, timeout: Duration, subprotocol: Option<&str>) -> ProbeStatus {
    let started = Instant::now();
    let result: Result<(), ProbeError> = async {
        let mut req = url
            .into_client_request()
            .map_err(|e| ProbeError::InvalidUrl(e.to_string()))?;
        if let Some(p) = subprotocol {
            req.headers_mut().insert(
                "Sec-WebSocket-Protocol",
                p.parse()
                    .map_err(|e: tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue| {
                        ProbeError::InvalidUrl(e.to_string())
                    })?,
            );
        }
        let (ws, _resp) = tokio::time::timeout(timeout, tokio_tungstenite::connect_async(req))
            .await
            .map_err(|_| ProbeError::InvalidUrl("ws connect timeout".into()))??;
        drop(ws);
        Ok(())
    }
    .await;

    let latency_ms = started.elapsed().as_millis();
    match result {
        Ok(()) => ProbeStatus {
            url: url.to_string(),
            reachable: true,
            latency_ms,
            detail: None,
        },
        Err(e) => ProbeStatus {
            url: url.to_string(),
            reachable: false,
            latency_ms,
            detail: Some(e.to_string()),
        },
    }
}

fn extract_typename(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("data")
                .and_then(|d| d.get("__typename"))
                .and_then(|t| t.as_str())
                .map(|s| format!("__typename={s}"))
        })
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}
