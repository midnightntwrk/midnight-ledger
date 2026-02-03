// This file is part of lunar-spark.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

/**
 * Formats a balance with proper denomination and SI prefixes.
 * Balances are stored in the smallest unit (6 decimal places).
 */
export function formatBalance(balance: bigint): string {
  const denomination = BigInt(10 ** 6);
  const value = balance / denomination;
  const fractionalPart = balance % denomination;

  // Use BigInt for thresholds to avoid precision loss
  const trillion = BigInt(1_000_000_000_000);
  const billion = BigInt(1_000_000_000);
  const million = BigInt(1_000_000);

  let result: string;

  if (value >= trillion) {
    const wholeTrillions = value / trillion;
    const remainderAfterTrillions = value % trillion;
    const decimalPart = Number((remainderAfterTrillions * 100n) / trillion);

    if (decimalPart === 0 && fractionalPart === 0n) {
      result = `${wholeTrillions.toLocaleString('en-US')}T`;
    } else {
      const decimalStr = (decimalPart / 100).toFixed(2).substring(1);
      result = `${wholeTrillions.toLocaleString('en-US')}${decimalStr.replace(/\.?0+$/, '')}T`;
    }
  } else if (value >= billion) {
    const wholeBillions = value / billion;
    const remainderAfterBillions = value % billion;
    const decimalPart = Number((remainderAfterBillions * 100n) / billion);

    if (decimalPart === 0 && fractionalPart === 0n) {
      result = `${wholeBillions.toLocaleString('en-US')}B`;
    } else {
      const decimalStr = (decimalPart / 100).toFixed(2).substring(1);
      result = `${wholeBillions.toLocaleString('en-US')}${decimalStr.replace(/\.?0+$/, '')}B`;
    }
  } else if (value >= million) {
    const wholeMillions = value / million;
    const remainderAfterMillions = value % million;
    const decimalPart = Number((remainderAfterMillions * 100n) / million);

    if (decimalPart === 0 && fractionalPart === 0n) {
      result = `${wholeMillions.toLocaleString('en-US')}M`;
    } else {
      const decimalStr = (decimalPart / 100).toFixed(2).substring(1);
      result = `${wholeMillions.toLocaleString('en-US')}${decimalStr.replace(/\.?0+$/, '')}M`;
    }
  } else {
    const wholeStr = value.toLocaleString('en-US');
    if (fractionalPart > 0n) {
      const fractionalStr = fractionalPart.toString().padStart(6, '0').replace(/0+$/, '');
      result = `${wholeStr}.${fractionalStr}`;
    } else {
      result = wholeStr;
    }
  }

  return result;
}

/** The native NIGHT token ID (all zeros) */
export const NIGHT_TOKEN_ID = '0000000000000000000000000000000000000000000000000000000000000000';

/**
 * Returns a human-readable display name for a token ID.
 */
export function getTokenDisplayName(tokenId: string): string {
  if (tokenId === NIGHT_TOKEN_ID) {
    return 'NIGHT';
  }
  return tokenId.substring(0, 8) + '...';
}

/**
 * Truncates a long address for display purposes.
 */
export function truncateAddress(address: string): string {
  if (address.length <= 40) return address;
  return address.substring(0, 16) + '...' + address.substring(address.length - 12);
}
