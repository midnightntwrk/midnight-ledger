//! Node JSON-RPC client. Phase-1 — raw substrate methods via
//! `jsonrpsee`. We swap to `subxt + midnight-node-metadata` when we
//! need typed extrinsic submission (iter-1 send step).

use jsonrpsee::core::client::ClientT;
use jsonrpsee::rpc_params;
use jsonrpsee::ws_client::WsClient;
use jsonrpsee::ws_client::WsClientBuilder;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::crypto::ensure_default_crypto_provider;
use crate::network::Network;

#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("ws-client: {0}")]
    Ws(#[from] jsonrpsee::core::client::Error),
}

/// Subset of substrate's `system_health` response we surface to the UI.
/// The full response also has `shouldHavePeers` and other fields; we
/// only decode what the wallet displays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeHealth {
    pub peers: u64,
    #[serde(rename = "isSyncing")]
    pub is_syncing: bool,
    #[serde(rename = "shouldHavePeers")]
    pub should_have_peers: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeStatus {
    pub health: NodeHealth,
    pub finalized_head_hash: String,
}

pub struct NodeClient {
    inner: WsClient,
}

impl NodeClient {
    pub async fn connect(network: Network) -> Result<Self, NodeError> {
        ensure_default_crypto_provider();
        let cfg = network.config();
        let inner = WsClientBuilder::default()
            .request_timeout(Duration::from_secs(15))
            .connection_timeout(Duration::from_secs(10))
            .build(cfg.node_ws_url)
            .await?;
        Ok(Self { inner })
    }

    pub async fn health(&self) -> Result<NodeHealth, NodeError> {
        Ok(self.inner.request("system_health", rpc_params![]).await?)
    }

    pub async fn finalized_head(&self) -> Result<String, NodeError> {
        Ok(self
            .inner
            .request("chain_getFinalizedHead", rpc_params![])
            .await?)
    }

    pub async fn status(&self) -> Result<NodeStatus, NodeError> {
        let (health, finalized_head_hash) =
            tokio::join!(self.health(), self.finalized_head());
        Ok(NodeStatus {
            health: health?,
            finalized_head_hash: finalized_head_hash?,
        })
    }
}
