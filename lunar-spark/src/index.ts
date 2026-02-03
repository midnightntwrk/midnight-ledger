// This file is part of lunar-spark.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

// Configuration
export {
  NetworkId,
  ENVIRONMENTS,
  ENVIRONMENT_OPTIONS,
  PROVING_SERVER_URL,
  getEnvironmentConfig,
  deriveIndexerWsUrl,
} from './config/environments';

export type { Environment, EnvironmentConfig } from './config/environments';

// Wallet initialization
export {
  initializeWallet,
  stopWallet,
} from './lib/wallet';

export type {
  WalletSecretKeys,
  WalletInitResult,
  UnshieldedKeystore,
} from './lib/wallet';

// Utilities
export {
  formatBalance,
  getTokenDisplayName,
  truncateAddress,
  NIGHT_TOKEN_ID,
} from './utils/balance';

export {
  calculateSyncPercentage,
  getSyncStatus,
  formatSyncProgress,
  getShieldedSyncProgress,
  getUnshieldedSyncProgress,
  getDustSyncProgress,
} from './utils/sync';

export type { WalletSyncProgress } from './utils/sync';

// Components
export { WalletDashboard } from './components/WalletDashboard';
