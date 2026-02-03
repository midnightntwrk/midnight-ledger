import React, {createContext, useContext, useState, useEffect, useCallback, useMemo, ReactNode} from 'react';
import {Alert} from 'react-native';
import type {Subscription} from 'rxjs';
import type {FacadeState} from '@midnight-ntwrk/wallet-sdk-facade';
import {ShieldedAddress, UnshieldedAddress} from '@midnight-ntwrk/wallet-sdk-address-format';
import {shieldedToken, unshieldedToken} from '@midnight-ntwrk/expo-midnight-ledger';
import {initializeWallet, stopWallet, type WalletInitResult} from '../lib/wallet';
import {type Environment, getEnvironmentConfig} from '../config/environments';
import {getShieldedSyncProgress, getUnshieldedSyncProgress, getDustSyncProgress} from '../utils/sync';

type WalletStatus = 'disconnected' | 'connecting' | 'connected' | 'error';
export type TransferWalletType = 'shielded' | 'unshielded';

// Simplified wallet state for UI consumption
export interface WalletSummary {
  address: string;
  balance: bigint;
  pendingBalance: bigint;
  availableCoins: number;
  pendingCoins: number;
  syncPercentage: number;
}

export interface WalletData {
  shielded: WalletSummary;
  unshielded: WalletSummary;
  dust: WalletSummary;
  totalBalance: bigint;
  isSynced: boolean;
}

interface WalletContextValue {
  status: WalletStatus;
  error: string | null;
  environment: Environment;
  facadeState: FacadeState | null;
  walletData: WalletData | null;
  networkId: string | null;
  connect: (seedHex: string) => Promise<void>;
  disconnect: () => Promise<void>;
  setEnvironment: (env: Environment) => void;
  transfer: (walletType: TransferWalletType, recipientAddress: string, amount: bigint) => Promise<void>;
}

const WalletContext = createContext<WalletContextValue | undefined>(undefined);

interface WalletProviderProps {
  children: ReactNode;
}

// Native NIGHT token type identifier
const NIGHT_TOKEN_TYPE = '0000000000000000000000000000000000000000000000000000000000000000';

// Helper to get NIGHT token balance from a balances map
function getNightBalance(balances: Record<string, bigint>): bigint {
  return balances[NIGHT_TOKEN_TYPE] ?? 0n;
}

export function WalletProvider({children}: WalletProviderProps) {
  const [status, setStatus] = useState<WalletStatus>('disconnected');
  const [error, setError] = useState<string | null>(null);
  const [environment, setEnvironment] = useState<Environment>('preprod');
  const [walletResult, setWalletResult] = useState<WalletInitResult | null>(null);
  const [facadeState, setFacadeState] = useState<FacadeState | null>(null);

  // Subscribe to wallet state when wallet is initialized
  useEffect(() => {
    if (!walletResult?.facade) return;

    const subscription: Subscription = walletResult.facade.state().subscribe({
      next: (newState) => {
        setFacadeState(newState);
      },
      error: (err) => {
        setError(`State update error: ${err}`);
        setStatus('error');
      },
    });

    return () => {
      subscription.unsubscribe();
    };
  }, [walletResult?.facade]);

  // Derive simplified wallet data from facade state
  const walletData = useMemo((): WalletData | null => {
    if (!facadeState || !walletResult?.networkId) return null;

    const networkId = walletResult.networkId;

    // Encode addresses
    const shieldedAddress = ShieldedAddress.codec.encode(networkId, facadeState.shielded.address).asString();
    const unshieldedAddress = UnshieldedAddress.codec.encode(networkId, facadeState.unshielded.address).asString();
    const dustAddress = facadeState.dust.dustAddress;

    // Get sync progress
    const shieldedSync = getShieldedSyncProgress(facadeState);
    const unshieldedSync = getUnshieldedSyncProgress(facadeState);
    const dustSync = getDustSyncProgress(facadeState);

    // Calculate NIGHT token balances
    const shieldedBalance = getNightBalance(facadeState.shielded.balances);
    const unshieldedBalance = getNightBalance(facadeState.unshielded.balances);
    const dustBalance = facadeState.dust.walletBalance(new Date());

    // Build wallet summaries
    const shielded: WalletSummary = {
      address: shieldedAddress,
      balance: shieldedBalance,
      pendingBalance: 0n, // TODO: Calculate pending from pendingCoins if needed
      availableCoins: facadeState.shielded.availableCoins.length,
      pendingCoins: facadeState.shielded.pendingCoins.length,
      syncPercentage: shieldedSync.percentage,
    };

    const unshielded: WalletSummary = {
      address: unshieldedAddress,
      balance: unshieldedBalance,
      pendingBalance: 0n,
      availableCoins: facadeState.unshielded.availableCoins.length,
      pendingCoins: facadeState.unshielded.pendingCoins.length,
      syncPercentage: unshieldedSync.percentage,
    };

    const dust: WalletSummary = {
      address: dustAddress,
      balance: dustBalance,
      pendingBalance: 0n,
      availableCoins: facadeState.dust.availableCoins.length,
      pendingCoins: facadeState.dust.pendingCoins.length,
      syncPercentage: dustSync.percentage,
    };

    const totalBalance = unshieldedBalance;
    const isSynced = facadeState.isSynced;

    return {
      shielded,
      unshielded,
      dust,
      totalBalance,
      isSynced,
    };
  }, [facadeState, walletResult?.networkId]);

  const connect = useCallback(
    async (seedHex: string) => {
      if (!seedHex || seedHex.length !== 64) {
        Alert.alert('Invalid Seed', 'Please enter a valid 32-byte seed as 64 hex characters');
        return;
      }

      setStatus('connecting');
      setError(null);

      try {
        // Convert hex to Uint8Array
        const seed = new Uint8Array(32);
        for (let i = 0; i < 32; i++) {
          seed[i] = parseInt(seedHex.substring(i * 2, i * 2 + 2), 16);
        }

        const envConfig = getEnvironmentConfig(environment);
        const result = await initializeWallet(seed, envConfig);

        setWalletResult(result);
        setStatus('connected');
      } catch (err) {
        console.log(err);
        const message = err instanceof Error ? err.message : String(err);
        setError(`Failed to initialize wallet: ${message}`);
        setStatus('error');
      }
    },
    [environment]
  );

  const disconnect = useCallback(async () => {
    if (walletResult) {
      try {
        await stopWallet(walletResult);
      } catch (err) {
        console.warn('Error stopping wallet:', err);
      }
    }
    setWalletResult(null);
    setFacadeState(null);
    setStatus('disconnected');
    setError(null);
  }, [walletResult]);

  const transfer = useCallback(
    async (walletType: TransferWalletType, recipientAddress: string, amount: bigint) => {
      if (!walletResult) {
        throw new Error('Wallet not connected');
      }

      const {facade, secretKeys, unshieldedKeystore} = walletResult;

      console.log('unshielded token', unshieldedToken);
      console.log('receiveraddress', recipientAddress);
      // Create the transfer transaction
      const recipe = await facade.transferTransaction(
        [
          {
            type: 'unshielded' as const,
            outputs: [
              {
                amount: 1n * 10n ** 6n,
                receiverAddress: recipientAddress,
                type: '0000000000000000000000000000000000000000000000000000000000000000',
              },
            ],
          },
        ],
        {
          shieldedSecretKeys: secretKeys.shieldedSecretKeys,
          dustSecretKey: secretKeys.dustSecretKey,
        },
        {
          ttl: new Date(Date.now() + 30 * 60 * 1000), // 30 minute TTL
        }
      );

      // If unshielded transfer, we need to sign the recipe first
      let finalizedTransaction;
      if (walletType === 'unshielded') {
        const signedRecipe = await facade.signRecipe(recipe, (payload: Uint8Array) =>
          unshieldedKeystore.signData(payload)
        );
        finalizedTransaction = await facade.finalizeRecipe(signedRecipe);
      } else {
        // Shielded transfers don't need signing
        finalizedTransaction = await facade.finalizeRecipe(recipe);
      }

      // Submit the transaction
      await facade.submitTransaction(finalizedTransaction);

      console.log('[transfer] Transaction submitted successfully');
    },
    [walletResult]
  );

  const value: WalletContextValue = {
    status,
    error,
    environment,
    facadeState,
    walletData,
    networkId: walletResult?.networkId ?? null,
    connect,
    disconnect,
    setEnvironment,
    transfer,
  };

  return <WalletContext.Provider value={value}>{children}</WalletContext.Provider>;
}

export function useWallet() {
  const context = useContext(WalletContext);
  if (context === undefined) {
    throw new Error('useWallet must be used within a WalletProvider');
  }
  return context;
}
