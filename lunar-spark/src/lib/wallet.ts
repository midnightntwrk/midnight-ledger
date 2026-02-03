// This file is part of lunar-spark.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

console.log('[wallet.ts] Module loading started...');

import {ZswapSecretKeys, DustSecretKey, LedgerParameters} from '@midnight-ntwrk/expo-midnight-ledger';
import {DustWallet} from '@midnight-ntwrk/wallet-sdk-dust-wallet';
import {WalletFacade} from '@midnight-ntwrk/wallet-sdk-facade';
import {HDWallet, Roles} from '@midnight-ntwrk/wallet-sdk-hd';
import {ShieldedWallet} from '@midnight-ntwrk/wallet-sdk-shielded';
import {
  createKeystore,
  InMemoryTransactionHistoryStorage,
  PublicKey,
  UnshieldedWallet,
} from '@midnight-ntwrk/wallet-sdk-unshielded-wallet';
import {NetworkId} from '@midnight-ntwrk/wallet-sdk-abstractions';
import type {EnvironmentConfig} from '../config/environments';

// Debug: Check what we got from imports
console.log('[wallet.ts] Checking imports...');
console.log('[wallet.ts] expo-midnight-ledger:', {
  ZswapSecretKeys: typeof ZswapSecretKeys,
  DustSecretKey: typeof DustSecretKey,
  LedgerParameters: typeof LedgerParameters,
});
console.log('[wallet.ts] wallet-sdk-dust-wallet:', {DustWallet: typeof DustWallet});
console.log('[wallet.ts] wallet-sdk-facade:', {WalletFacade: typeof WalletFacade});
console.log('[wallet.ts] wallet-sdk-hd:', {HDWallet: typeof HDWallet, Roles});
console.log('[wallet.ts] wallet-sdk-shielded:', {ShieldedWallet: typeof ShieldedWallet});
console.log('[wallet.ts] wallet-sdk-unshielded-wallet:', {
  createKeystore: typeof createKeystore,
  InMemoryTransactionHistoryStorage: typeof InMemoryTransactionHistoryStorage,
  PublicKey: typeof PublicKey,
  UnshieldedWallet: typeof UnshieldedWallet,
});
console.log('[wallet.ts] wallet-sdk-abstractions:', {NetworkId});
console.log('[wallet.ts] All imports loaded successfully!');

/**
 * Secret keys for wallet operations
 */
export interface WalletSecretKeys {
  shieldedSecretKeys: ZswapSecretKeys;
  dustSecretKey: DustSecretKey;
}

/**
 * Keystore type for unshielded wallet
 */
export type UnshieldedKeystore = ReturnType<typeof createKeystore>;

/**
 * Result of wallet initialization
 */
export interface WalletInitResult {
  /** The wallet facade orchestrating all wallets */
  facade: WalletFacade;
  /** The network ID for address encoding */
  networkId: NetworkId.NetworkId;
  /** Secret keys for transaction signing */
  secretKeys: WalletSecretKeys;
  /** Keystore for unshielded wallet operations */
  unshieldedKeystore: UnshieldedKeystore;
}

/**
 * Initialize and start the wallet facade with all three wallets.
 *
 * This function follows the same pattern as wallet-cli:
 * 1. Initialize HD wallet from seed
 * 2. Derive keys for all three wallet types (Zswap, NightExternal, Dust)
 * 3. Create secret keys for each wallet type
 * 4. Initialize each wallet (Shielded, Unshielded, Dust)
 * 5. Create and start the WalletFacade
 *
 * @param seed - The wallet seed as a Uint8Array (typically 64 bytes from BIP39)
 * @param envConfig - The environment configuration
 * @returns The initialized and started WalletFacade with secret keys
 *
 * @example
 * ```typescript
 * import { initializeWallet } from './lib/wallet';
 * import { getEnvironmentConfig } from './config/environments';
 *
 * const seed = await generateSeedFromMnemonic(mnemonic);
 * const config = getEnvironmentConfig('preprod');
 * const { facade, secretKeys } = await initializeWallet(seed, config);
 *
 * // Subscribe to wallet state
 * facade.state().subscribe({
 *   next: (state) => console.log('Wallet state:', state),
 * });
 *
 * // Clean up when done
 * await facade.stop();
 * secretKeys.shieldedSecretKeys.clear();
 * secretKeys.dustSecretKey.clear();
 * ```
 */
export async function initializeWallet(seed: Uint8Array, envConfig: EnvironmentConfig): Promise<WalletInitResult> {
  console.log('[initializeWallet] Starting wallet initialization...');
  console.log('[initializeWallet] Seed length:', seed.length);

  // Step 1: Initialize HD wallet from seed
  console.log('[initializeWallet] Step 1: Creating HD wallet from seed...');
  const hdWallet = HDWallet.fromSeed(seed);
  console.log('[initializeWallet] HD wallet result type:', hdWallet.type);

  if (hdWallet.type !== 'seedOk') {
    throw new Error('Failed to initialize HDWallet from seed');
  }

  // Step 2: Derive keys for all three wallet types
  console.log('[initializeWallet] Step 2: Deriving keys for all wallet types...');
  const derivationResult = hdWallet.hdWallet
    .selectAccount(0)
    .selectRoles([Roles.Zswap, Roles.NightExternal, Roles.Dust])
    .deriveKeysAt(0);
  console.log('[initializeWallet] Derivation result type:', derivationResult.type);

  if (derivationResult.type !== 'keysDerived') {
    throw new Error('Failed to derive keys from HD wallet');
  }

  // Clear sensitive data from HD wallet
  console.log('[initializeWallet] Clearing HD wallet...');
  hdWallet.hdWallet.clear();

  // Step 3: Create secret keys for each wallet type using native module
  console.log('[initializeWallet] Step 3: Creating secret keys...');
  console.log('[initializeWallet] Creating ZswapSecretKeys...');
  const shieldedSecretKeys = ZswapSecretKeys.fromSeed(derivationResult.keys[Roles.Zswap]);
  console.log('[initializeWallet] ZswapSecretKeys created:', shieldedSecretKeys);

  console.log('[initializeWallet] Creating DustSecretKey...');
  const dustSecretKey = DustSecretKey.fromSeed(derivationResult.keys[Roles.Dust]);
  console.log('[initializeWallet] DustSecretKey created:', dustSecretKey);

  console.log('[initializeWallet] Creating unshielded keystore...');
  const unshieldedKeystore = createKeystore(derivationResult.keys[Roles.NightExternal], envConfig.networkId);
  console.log('[initializeWallet] Unshielded keystore created:', unshieldedKeystore);

  // Step 4: Create wallet configurations
  console.log('[initializeWallet] Step 4: Creating wallet config...');
  const config = {
    networkId: envConfig.networkId,
    indexerClientConnection: {
      indexerHttpUrl: envConfig.indexerHttpUrl,
      indexerWsUrl: envConfig.indexerWsUrl,
    },
    provingServerUrl: new URL(envConfig.provingServerUrl),
    relayURL: new URL(envConfig.nodeWsUrl),
    costParameters: {
      additionalFeeOverhead: 300_000_000_000_000_000n,
      feeBlocksMargin: 5,
    },
  };
  console.log(
    JSON.stringify(config, (key, value) => {
      if (key === 'additionalFeeOverhead') {
        return Number(value);
      }

      return value;
    })
  );
  console.log('[initializeWallet] Config created');

  // Get initial dust parameters from ledger
  console.log('[initializeWallet] Getting ledger parameters...');
  const ledgerParams = LedgerParameters.initialParameters();
  console.log('[initializeWallet] Ledger params:', ledgerParams);
  const dustParamsId = ledgerParams.getDustParamsId();
  console.log('[initializeWallet] Dust params ID:', dustParamsId);

  // Step 5: Initialize each wallet
  console.log('[initializeWallet] Step 5: Initializing wallets...');

  console.log('[initializeWallet] Creating ShieldedWallet...');
  const shieldedWallet = ShieldedWallet(config).startWithSecretKeys(shieldedSecretKeys);
  console.log('[initializeWallet] ShieldedWallet created');

  console.log('[initializeWallet] Creating DustWallet...');
  console.log('[initializeWallet] DustWallet function:', typeof DustWallet);
  let dustWallet;
  try {
    console.log('[initializeWallet] Calling DustWallet(config)...');
    const DustWalletClass = DustWallet(config);
    console.log('[initializeWallet] DustWalletClass:', DustWalletClass);
    console.log('[initializeWallet] DustWalletClass.startWithSecretKey:', typeof DustWalletClass?.startWithSecretKey);
    console.log('[initializeWallet] Calling startWithSecretKey...');
    dustWallet = DustWalletClass.startWithSecretKey(
      dustSecretKey,
      dustParamsId as unknown as Parameters<typeof DustWalletClass.startWithSecretKey>[1]
    );
    console.log('[initializeWallet] DustWallet created');
  } catch (err) {
    console.log('[initializeWallet] DustWallet ERROR:', err);
    console.log('[initializeWallet] Error stack:', (err as Error).stack);
    throw err;
  }

  console.log('[initializeWallet] Creating UnshieldedWallet...');
  const unshieldedWallet = UnshieldedWallet({
    ...config,
    txHistoryStorage: new InMemoryTransactionHistoryStorage(),
  }).startWithPublicKey(PublicKey.fromKeyStore(unshieldedKeystore));
  console.log('[initializeWallet] UnshieldedWallet created');

  // Clean up ledger params
  console.log('[initializeWallet] Disposing ledger params...');
  ledgerParams.dispose();

  // Step 6: Create and start the wallet facade
  console.log('[initializeWallet] Step 6: Creating WalletFacade...');
  const facade = new WalletFacade(shieldedWallet, unshieldedWallet, dustWallet);
  console.log('[initializeWallet] WalletFacade created, starting...');
  await facade.start(shieldedSecretKeys, dustSecretKey);
  console.log('[initializeWallet] WalletFacade started successfully!');

  return {
    facade,
    networkId: envConfig.networkId,
    secretKeys: {shieldedSecretKeys, dustSecretKey},
    unshieldedKeystore,
  };
}

/**
 * Stop the wallet facade and clean up resources.
 *
 * @param result - The wallet initialization result
 */
export async function stopWallet(result: WalletInitResult): Promise<void> {
  await result.facade.stop();
  result.secretKeys.shieldedSecretKeys.clear();
  result.secretKeys.dustSecretKey.clear();
}
