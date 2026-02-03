// This file is part of lunar-spark.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

/**
 * Network identifier enum matching the wallet SDK's NetworkId
 */
export enum NetworkId {
  Undeployed = 'undeployed',
  DevNet = 'devnet',
  QaNet = 'qanet',
  Preview = 'preview',
  PreProd = 'preprod',
  MainNet = 'mainnet',
}

/**
 * Environment type for network selection
 */
export type Environment = 'preprod' | 'preview' | 'qanet' | 'dev' | 'undeployed';

/**
 * Configuration for a specific network environment
 */
export interface EnvironmentConfig {
  /** Network identifier */
  networkId: NetworkId;
  /** HTTP GraphQL endpoint for the indexer */
  indexerHttpUrl: string;
  /** WebSocket GraphQL endpoint for the indexer */
  indexerWsUrl: string;
  /** WebSocket RPC endpoint for the node */
  nodeWsUrl: string;
  /** URL for the proving server (typically localhost) */
  provingServerUrl: string;
}

/**
 * Default proving server URL (runs locally)
 * Users should start a proving server instance before using the wallet
 */
export const PROVING_SERVER_URL = 'http://localhost:6300';

/**
 * Environment options for UI selection
 */
export const ENVIRONMENT_OPTIONS: Array<{label: string; value: Environment}> = [
  {label: 'PreProd', value: 'preprod'},
  {label: 'Preview', value: 'preview'},
  {label: 'QANet', value: 'qanet'},
  {label: 'DevNet', value: 'dev'},
  {label: 'Undeployed', value: 'undeployed'},
];

/**
 * Derive the WebSocket URL from an HTTP URL
 * Converts https:// to wss:// (or http:// to ws://) and appends /ws
 */
export function deriveIndexerWsUrl(httpUrl: string): string {
  const wsUrl = httpUrl.replace(/^https:\/\//, 'wss://').replace(/^http:\/\//, 'ws://');
  return wsUrl.endsWith('/ws') ? wsUrl : `${wsUrl}/ws`;
}

/**
 * Environment configurations for all supported networks
 */
export const ENVIRONMENTS: Record<Environment, EnvironmentConfig> = {
  preprod: {
    networkId: NetworkId.PreProd,
    indexerHttpUrl: 'https://indexer.preprod.midnight.network/api/v3/graphql',
    indexerWsUrl: 'wss://indexer.preprod.midnight.network/api/v3/graphql/ws',
    nodeWsUrl: 'wss://rpc.preprod.midnight.network',
    provingServerUrl: PROVING_SERVER_URL,
  },
  preview: {
    networkId: NetworkId.Preview,
    indexerHttpUrl: 'https://indexer.preview.midnight.network/api/v3/graphql',
    indexerWsUrl: 'wss://indexer.preview.midnight.network/api/v3/graphql/ws',
    nodeWsUrl: 'wss://rpc.preview.midnight.network',
    provingServerUrl: PROVING_SERVER_URL,
  },
  qanet: {
    networkId: NetworkId.QaNet,
    indexerHttpUrl: 'https://indexer.qanet.dev.midnight.network/api/v3/graphql',
    indexerWsUrl: 'wss://indexer.qanet.dev.midnight.network/api/v3/graphql/ws',
    nodeWsUrl: 'wss://rpc.qanet.dev.midnight.network',
    provingServerUrl: PROVING_SERVER_URL,
  },
  dev: {
    networkId: NetworkId.DevNet,
    indexerHttpUrl: 'https://indexer.devnet.midnight.network/api/v3/graphql',
    indexerWsUrl: 'wss://indexer.devnet.midnight.network/api/v3/graphql/ws',
    nodeWsUrl: 'wss://rpc.devnet.midnight.network',
    provingServerUrl: PROVING_SERVER_URL,
  },
  undeployed: {
    networkId: NetworkId.Undeployed,
    indexerHttpUrl: 'http://localhost:8088/api/v3/graphql',
    indexerWsUrl: 'ws://localhost:8088/api/v3/graphql/ws',
    nodeWsUrl: 'ws://localhost:9944',
    provingServerUrl: PROVING_SERVER_URL,
  },
};

/**
 * Get the environment configuration for a given environment name
 */
export function getEnvironmentConfig(environment: Environment): EnvironmentConfig {
  return ENVIRONMENTS[environment];
}
