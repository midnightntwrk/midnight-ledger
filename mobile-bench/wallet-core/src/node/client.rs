//! Node JSON-RPC client. Phase-1 — raw substrate methods via
//! `jsonrpsee`. Phase-2 adds a `subxt::OnlineClient` alongside for
//! typed extrinsic submission (`Midnight.send_mn_transaction`).

use jsonrpsee::core::client::ClientT;
use jsonrpsee::rpc_params;
use jsonrpsee::ws_client::WsClient;
use jsonrpsee::ws_client::WsClientBuilder;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use subxt::{OnlineClient, SubstrateConfig};

use crate::crypto::ensure_default_crypto_provider;
use crate::network::Network;

#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("ws-client: {0}")]
    Ws(#[from] jsonrpsee::core::client::Error),
    #[error("subxt: {0}")]
    Subxt(String),
    #[error("submit: {0}")]
    Submit(String),
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

/// Outcome of a successful in-block submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitResult {
    pub tx_hash: [u8; 32],
    pub block_hash: [u8; 32],
}

pub struct NodeClient {
    inner: WsClient,
    subxt: OnlineClient<SubstrateConfig>,
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
        let subxt = OnlineClient::<SubstrateConfig>::from_url(cfg.node_ws_url)
            .await
            .map_err(|e| NodeError::Subxt(e.to_string()))?;
        Ok(Self { inner, subxt })
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

    /// Submit a SCALE-encoded Midnight transaction via the
    /// `Midnight.send_mn_transaction(bytes)` runtime call, then
    /// wait for it to be included in a block. Does NOT wait for
    /// finality — that's a design choice (see deploy-submit spec).
    ///
    /// The pallet declares `send_mn_transaction` as an unsigned
    /// extrinsic (validated via `ValidateUnsigned`). We submit
    /// **unsigned** — wrapping in a substrate envelope-signed
    /// extrinsic causes the node to reject with
    /// `Invalid Transaction (1010)`. The `_signer` arg is kept on
    /// the signature for forward-compat with future signed
    /// extrinsics; today it's unused. Mirrors the upstream
    /// toolkit's `sender.rs` create_unsigned() pattern.
    #[allow(dead_code)] // Wired by Wallet::create_did in Task 11.
    pub async fn submit_deploy(
        &self,
        bytes: Vec<u8>,
        _signer: &crate::MidnightSigner,
    ) -> Result<SubmitResult, NodeError> {
        use midnight_node_metadata::midnight_metadata_latest as runtime;

        use subxt::tx::TxStatus;

        let call = runtime::tx().midnight().send_mn_transaction(bytes);
        let unsigned = self
            .subxt
            .tx()
            .create_unsigned(&call)
            .map_err(|e| NodeError::Submit(format!("create_unsigned: {e}")))?;
        let mut progress = unsigned
            .submit_and_watch()
            .await
            .map_err(|e| NodeError::Submit(e.to_string()))?;

        // subxt 0.44 doesn't have a one-shot wait_for_in_block —
        // we drive the status stream and break on the first
        // `InBestBlock` / `InFinalizedBlock` variant. (Finalized is
        // strictly stronger; we accept either.)
        let in_block = loop {
            match progress.next().await {
                Some(Ok(TxStatus::InBestBlock(b))) | Some(Ok(TxStatus::InFinalizedBlock(b))) => {
                    break b;
                }
                Some(Ok(TxStatus::Invalid { message }))
                | Some(Ok(TxStatus::Dropped { message }))
                | Some(Ok(TxStatus::Error { message })) => {
                    return Err(NodeError::Submit(message));
                }
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(NodeError::Submit(e.to_string())),
                None => return Err(NodeError::Submit("tx status stream ended early".into())),
            }
        };
        in_block
            .wait_for_success()
            .await
            .map_err(|e| NodeError::Submit(format!("wait_for_success: {e}")))?;

        Ok(SubmitResult {
            tx_hash: in_block.extrinsic_hash().0,
            block_hash: in_block.block_hash().0,
        })
    }
}
