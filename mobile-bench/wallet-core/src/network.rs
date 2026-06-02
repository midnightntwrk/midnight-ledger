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
    /// Localhost standalone — `http://localhost:1{8088,9944,6300}`.
    /// Matches gsd-wallet's "Undeployed" preset and is the default
    /// for any developer running the docker-compose standalone
    /// alongside the simulator (no env-var setup required).
    Undeployed,
    /// Same standalone chain as [`Network::Undeployed`], but reached
    /// over Yurii's tailnet (`100.110.241.102:1{8088,9944,6300}`).
    /// Lets the phone APK target the laptop's docker-hosted standalone
    /// without changing build flags. `network_id`, address prefix and
    /// the pre-funded genesis seed are identical to `Undeployed` —
    /// the only differences are the endpoint URLs.
    UndeployedYurii,
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
    pub const ALL: [Network; 7] = [
        Network::Mainnet,
        Network::PreProd,
        Network::Preview,
        Network::QaNet,
        Network::DevNet,
        Network::Undeployed,
        Network::UndeployedYurii,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Network::Mainnet => "Mainnet",
            Network::PreProd => "PreProd",
            Network::Preview => "Preview",
            Network::QaNet => "QANet",
            Network::DevNet => "DevNet",
            Network::Undeployed => "Undeployed",
            Network::UndeployedYurii => "Undeployed (Yurii's)",
        }
    }

    /// True for any variant that targets the standalone (no-deploy)
    /// chain. Used by call sites that branch on "is this the local
    /// docker-compose chain?" — the funded genesis seed, the address
    /// prefix `mn_*_undeployed`, and the network-id baked into tx
    /// signatures all share the same value for both
    /// [`Network::Undeployed`] (localhost) and
    /// [`Network::UndeployedYurii`] (tailscale). The variants differ
    /// only in how the wallet reaches the chain.
    pub fn is_undeployed(self) -> bool {
        matches!(self, Network::Undeployed | Network::UndeployedYurii)
    }

    /// Case-insensitive lookup against either `label()`
    /// ("PreProd") or `config().network_id` ("preprod"). Used
    /// by the backup-file importer (`store::backup`) to parse
    /// the human-readable `network` field back into a
    /// `Network` discriminant.
    pub fn from_label(s: &str) -> Option<Network> {
        let needle = s.trim().to_lowercase();
        Network::ALL.into_iter().find(|n| {
            n.label().to_lowercase() == needle
                || n.config().network_id.to_lowercase() == needle
        })
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
                // Standalone Midnight env, host-port-shifted by +10000
                // (2026-05-27) to evade collision with a parallel
                // midnight task that occupies the default 9944 / 8088
                // / 6300 set. The docker-compose macOS overlay at
                // `/tmp/midnight-standalone/docker-compose.macos.yml`
                // does the host→container remap.
                //
                // Strict localhost — no env-var override. Pick this
                // variant when the wallet runs alongside the docker
                // env on the same host (desktop dev, simulator, etc.).
                // For reaching the chain from another device, use
                // [`Network::UndeployedYurii`] (tailscale) or add a
                // sibling variant with your own endpoints.
                network_id: "undeployed",
                indexer_http_url: "http://localhost:18088/api/v4/graphql",
                indexer_ws_url: "ws://localhost:18088/api/v4/graphql/ws",
                node_ws_url: "ws://localhost:19944",
                proving_server_url: "http://localhost:16300",
            },
            Network::UndeployedYurii => NetworkConfig {
                // Same standalone chain as [`Network::Undeployed`],
                // reached over Yurii's tailnet so the phone APK can
                // talk to the laptop-hosted docker chain without
                // changing build flags. The tailscale IP is the
                // laptop side (`yuriys-macbook-pro`,
                // `100.110.241.102`). Ports stay shifted +10000.
                //
                // `network_id` matches `Undeployed` because it's the
                // SAME chain — txs signed by either variant are
                // accepted by the other, and the funded genesis seed
                // applies to both. Only the URLs differ.
                network_id: "undeployed",
                indexer_http_url: "http://100.110.241.102:18088/api/v4/graphql",
                indexer_ws_url: "ws://100.110.241.102:18088/api/v4/graphql/ws",
                node_ws_url: "ws://100.110.241.102:19944",
                proving_server_url: "http://100.110.241.102:16300",
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
