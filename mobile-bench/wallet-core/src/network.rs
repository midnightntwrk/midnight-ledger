use serde::{Deserialize, Serialize};

/// One of Midnight's deployed environments. URLs mirror gsd-wallet's
/// `src/shared/environments.ts` exactly so a wallet can talk to the same
/// hosts that gsd-wallet talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Mainnet,
    PreProd,
    Preview,
    QaNet,
    DevNet,
    /// Localhost — matches gsd-wallet's "Undeployed" preset.
    Undeployed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Lowercase network id used by `LedgerState::network_id` and tx
    /// construction. Matches gsd-wallet's `NetworkId` enum strings.
    pub network_id: &'static str,
    pub indexer_http_url: &'static str,
    pub indexer_ws_url: &'static str,
    pub node_ws_url: &'static str,
    /// The proof server is host-local in gsd-wallet's defaults; we keep
    /// the same convention here. Override per-wallet later.
    pub proving_server_url: &'static str,
}

impl Network {
    pub const ALL: [Network; 6] = [
        Network::Mainnet,
        Network::PreProd,
        Network::Preview,
        Network::QaNet,
        Network::DevNet,
        Network::Undeployed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Network::Mainnet => "Mainnet",
            Network::PreProd => "PreProd",
            Network::Preview => "Preview",
            Network::QaNet => "QANet",
            Network::DevNet => "DevNet",
            Network::Undeployed => "Undeployed",
        }
    }

    pub fn config(self) -> NetworkConfig {
        match self {
            Network::Mainnet => NetworkConfig {
                network_id: "mainnet",
                indexer_http_url: "https://indexer.mainnet.midnight.network/api/v4/graphql",
                indexer_ws_url: "wss://indexer.mainnet.midnight.network/api/v4/graphql/ws",
                node_ws_url: "wss://rpc.mainnet.midnight.network",
                proving_server_url: "http://localhost:6300",
            },
            Network::PreProd => NetworkConfig {
                network_id: "preprod",
                indexer_http_url: "https://indexer.preprod.midnight.network/api/v4/graphql",
                indexer_ws_url: "wss://indexer.preprod.midnight.network/api/v4/graphql/ws",
                node_ws_url: "wss://rpc.preprod.midnight.network",
                proving_server_url: "http://localhost:6300",
            },
            Network::Preview => NetworkConfig {
                network_id: "preview",
                indexer_http_url: "https://indexer.preview.midnight.network/api/v4/graphql",
                indexer_ws_url: "wss://indexer.preview.midnight.network/api/v4/graphql/ws",
                node_ws_url: "wss://rpc.preview.midnight.network",
                proving_server_url: "http://localhost:6300",
            },
            Network::QaNet => NetworkConfig {
                network_id: "qanet",
                indexer_http_url: "https://indexer.qanet.midnight.network/api/v4/graphql",
                indexer_ws_url: "wss://indexer.qanet.midnight.network/api/v4/graphql/ws",
                node_ws_url: "wss://rpc.qanet.midnight.network",
                proving_server_url: "http://localhost:6300",
            },
            Network::DevNet => NetworkConfig {
                network_id: "devnet",
                indexer_http_url: "https://indexer.devnet.midnight.network/api/v4/graphql",
                indexer_ws_url: "wss://indexer.devnet.midnight.network/api/v4/graphql/ws",
                node_ws_url: "wss://rpc.devnet.midnight.network",
                proving_server_url: "http://localhost:6300",
            },
            Network::Undeployed => NetworkConfig {
                network_id: "undeployed",
                indexer_http_url: "http://localhost:8088/api/v4/graphql",
                indexer_ws_url: "ws://localhost:8088/api/v4/graphql/ws",
                node_ws_url: "ws://localhost:9944",
                proving_server_url: "http://localhost:6300",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_network_has_distinct_indexer_url() {
        let mut seen = std::collections::HashSet::new();
        for n in Network::ALL {
            assert!(
                seen.insert(n.config().indexer_http_url),
                "duplicate indexer URL for {n:?}"
            );
        }
    }

    #[test]
    fn preprod_urls_match_gsd_wallet() {
        let cfg = Network::PreProd.config();
        assert_eq!(
            cfg.indexer_http_url,
            "https://indexer.preprod.midnight.network/api/v4/graphql"
        );
        assert_eq!(cfg.node_ws_url, "wss://rpc.preprod.midnight.network");
    }
}
