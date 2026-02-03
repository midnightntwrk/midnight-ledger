// This file is part of lunar-spark.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import type { FacadeState } from '@midnight-ntwrk/wallet-sdk-facade';

/**
 * Sync progress information for a single wallet
 */
export interface WalletSyncProgress {
  applied: number;
  total: number;
  percentage: number;
}

/**
 * Get sync progress for the shielded wallet
 */
export function getShieldedSyncProgress(state: FacadeState): WalletSyncProgress {
  const progress = state.shielded.progress;
  const applied = Number(progress.appliedIndex ?? 0);
  const total = Number(progress.highestRelevantIndex ?? 0);
  const percentage = total === 0 ? 100 : Math.floor((applied / total) * 100);
  return { applied, total, percentage };
}

/**
 * Get sync progress for the unshielded wallet
 */
export function getUnshieldedSyncProgress(state: FacadeState): WalletSyncProgress {
  const progress = state.unshielded.progress;
  const applied = Number(progress.appliedId);
  const total = Number(progress.highestTransactionId);
  const percentage = total === 0 ? 100 : Math.floor((applied / total) * 100);
  return { applied, total, percentage };
}

/**
 * Get sync progress for the dust wallet
 */
export function getDustSyncProgress(state: FacadeState): WalletSyncProgress {
  const progress = state.dust.progress;
  const applied = Number(progress.appliedIndex ?? 0);
  const total = Number(progress.highestRelevantIndex ?? 0);
  const percentage = total === 0 ? 100 : Math.floor((applied / total) * 100);
  return { applied, total, percentage };
}

/**
 * Calculate the overall sync percentage from all three wallets
 */
export function calculateSyncPercentage(state: FacadeState): number {
  const shielded = getShieldedSyncProgress(state);
  const unshielded = getUnshieldedSyncProgress(state);
  const dust = getDustSyncProgress(state);

  const overall = (shielded.percentage + unshielded.percentage + dust.percentage) / 3;
  return Math.floor(overall);
}

/**
 * Get sync status string with percentage
 */
export function getSyncStatus(state: FacadeState): string {
  const percentage = calculateSyncPercentage(state);

  if (state.isSynced || percentage === 100) {
    return 'Synced';
  }

  return `Syncing (${percentage}%)`;
}

/**
 * Format sync progress as "current/total (percentage%)"
 */
export function formatSyncProgress(progress: WalletSyncProgress): string {
  return `${progress.applied}/${progress.total} (${progress.percentage}%)`;
}

/**
 * Get sync status text from percentage
 */
export function getSyncStatusText(percentage: number): string {
  if (percentage === 100) {
    return 'Synced';
  }
  return 'Syncing...';
}
