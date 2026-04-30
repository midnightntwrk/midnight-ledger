//! Indexer client. Schema + queries are vendored from
//! [midnightntwrk/midnight-indexer](https://github.com/midnightntwrk/midnight-indexer)
//! so we use the maintainers' own toolchain (`graphql_client` codegen
//! against `schema-v4.graphql`).

use graphql_client::{GraphQLQuery, Response};
use serde::{Deserialize, Serialize};

use crate::crypto::ensure_default_crypto_provider;
use crate::network::Network;

/// Indexer scalars are wrapped strings. graphql_client requires every
/// custom scalar referenced by an *enabled* query to have a Rust type
/// alias in scope; we add them lazily as queries land.
type HexEncoded = String;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "queries/midnight-indexer/schema-v4.graphql",
    query_path = "queries/chain_tip.graphql",
    response_derives = "Debug, Clone, Serialize, Deserialize"
)]
struct ChainTip;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "queries/midnight-indexer/schema-v4.graphql",
    query_path = "queries/contract_state.graphql",
    response_derives = "Debug, Clone, Serialize, Deserialize"
)]
struct ContractState;

#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("graphql errors: {0}")]
    GraphQl(String),
    #[error("empty response — neither data nor errors returned")]
    EmptyResponse,
}

/// What the wallet shows as the "chain tip" pill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainTipInfo {
    pub hash: String,
    pub height: i64,
    pub protocol_version: i64,
    pub timestamp_unix: i64,
    pub author_hex: Option<String>,
}

/// Snapshot of a contract's on-chain state, as last known to the
/// indexer. `state_hex` is the SCALE/serialize-encoded
/// `onchain_state::ContractState` payload — DID-specific decoding
/// happens in `wallet_core::did::contract`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractStateInfo {
    pub address_hex: String,
    pub state_hex: String,
    pub last_tx_hash: String,
    pub last_block_height: Option<i64>,
}

#[derive(Clone)]
pub struct IndexerClient {
    http: reqwest::Client,
    http_url: String,
}

impl IndexerClient {
    pub fn new(network: Network) -> Result<Self, IndexerError> {
        ensure_default_crypto_provider();
        let cfg = network.config();
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        Ok(Self {
            http,
            http_url: cfg.indexer_http_url.to_string(),
        })
    }

    pub async fn chain_tip(&self) -> Result<Option<ChainTipInfo>, IndexerError> {
        let body = ChainTip::build_query(chain_tip::Variables {});
        let resp: Response<chain_tip::ResponseData> =
            self.http.post(&self.http_url).json(&body).send().await?.json().await?;

        if let Some(errs) = resp.errors {
            if !errs.is_empty() {
                return Err(IndexerError::GraphQl(
                    errs.iter().map(|e| e.message.clone()).collect::<Vec<_>>().join("; "),
                ));
            }
        }

        let data = resp.data.ok_or(IndexerError::EmptyResponse)?;
        Ok(data.block.map(|b| ChainTipInfo {
            hash: b.hash,
            height: b.height,
            protocol_version: b.protocol_version,
            timestamp_unix: b.timestamp,
            author_hex: b.author,
        }))
    }

    /// Fetch the latest contract action (deploy / call / update) for
    /// the given address. Returns `None` if the indexer doesn't know
    /// about the address (no transactions yet, or wrong network).
    pub async fn contract_state(
        &self,
        address_hex: &str,
    ) -> Result<Option<ContractStateInfo>, IndexerError> {
        let body = ContractState::build_query(contract_state::Variables {
            address: address_hex.to_string(),
        });
        let resp: Response<contract_state::ResponseData> =
            self.http.post(&self.http_url).json(&body).send().await?.json().await?;

        if let Some(errs) = resp.errors {
            if !errs.is_empty() {
                return Err(IndexerError::GraphQl(
                    errs.iter().map(|e| e.message.clone()).collect::<Vec<_>>().join("; "),
                ));
            }
        }

        let data = resp.data.ok_or(IndexerError::EmptyResponse)?;
        Ok(data.contract_action.map(|a| ContractStateInfo {
            address_hex: a.address,
            state_hex: a.state,
            last_tx_hash: a.transaction.hash,
            last_block_height: Some(a.transaction.block.height),
        }))
    }
}
