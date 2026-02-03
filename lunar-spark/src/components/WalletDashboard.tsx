// This file is part of lunar-spark.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import React, {useCallback} from 'react';
import {View, Text, StyleSheet, ScrollView, TouchableOpacity, Alert} from 'react-native';
import type {FacadeState} from '@midnight-ntwrk/wallet-sdk-facade';
import {ShieldedAddress, UnshieldedAddress} from '@midnight-ntwrk/wallet-sdk-address-format';
import {NetworkId} from '@midnight-ntwrk/wallet-sdk-abstractions';
import {formatBalance, getTokenDisplayName, truncateAddress, NIGHT_TOKEN_ID} from '../utils/balance';
import {
  getSyncStatus,
  getShieldedSyncProgress,
  getUnshieldedSyncProgress,
  getDustSyncProgress,
  formatSyncProgress,
} from '../utils/sync';

interface Props {
  state: FacadeState;
  networkId: NetworkId.NetworkId;
}

export const WalletDashboard: React.FC<Props> = ({state, networkId}) => {
  // Encode addresses
  const shieldedAddressStr = ShieldedAddress.codec.encode(networkId, state.shielded.address).asString();
  const unshieldedAddressStr = UnshieldedAddress.codec.encode(networkId, state.unshielded.address).asString();
  const dustAddressStr = state.dust.dustAddress;

  console.log(unshieldedAddressStr);
  const showAddress = useCallback((label: string, address: string) => {
    Alert.alert(label, address, [{text: 'OK'}], {userInterfaceStyle: 'dark'});
  }, []);

  // Get balances
  const shieldedBalances = Object.entries(state.shielded.balances);
  const unshieldedBalances = Object.entries(state.unshielded.balances);
  const dustBalance = state.dust.walletBalance(new Date());

  // Calculate total NIGHT balance (unshielded only)
  const totalNightBalance = state.unshielded.balances[NIGHT_TOKEN_ID] ?? 0n;

  // Get sync progress
  const syncStatus = getSyncStatus(state);
  const shieldedSync = getShieldedSyncProgress(state);
  const unshieldedSync = getUnshieldedSyncProgress(state);
  const dustSync = getDustSyncProgress(state);

  return (
    <ScrollView style={styles.container} contentContainerStyle={styles.content}>
      {/* Overall Status */}
      <View style={styles.statusBar}>
        <View style={[styles.statusDot, state.isSynced ? styles.statusSynced : styles.statusSyncing]} />
        <Text style={styles.statusText}>{syncStatus}</Text>
      </View>

      {/* Total NIGHT Balance */}
      <View style={styles.totalBalanceCard}>
        <Text style={styles.totalBalanceLabel}>Total Balance</Text>
        <Text style={styles.totalBalanceValue}>{formatBalance(totalNightBalance)}</Text>
        <Text style={styles.totalBalanceCurrency}>NIGHT</Text>
      </View>

      {/* Shielded Wallet */}
      <View style={styles.walletCard}>
        <View style={styles.walletHeader}>
          <View style={[styles.walletIndicator, styles.shieldedIndicator]} />
          <Text style={styles.walletTitle}>Shielded Wallet</Text>
        </View>

        <View style={styles.walletContent}>
          <Text style={styles.label}>Address</Text>
          <View style={styles.addressRow}>
            <Text style={styles.address} selectable numberOfLines={1}>
              {truncateAddress(shieldedAddressStr)}
            </Text>
            <TouchableOpacity
              style={styles.copyButton}
              onPress={() => showAddress('Shielded Address', shieldedAddressStr)}
            >
              <Text style={styles.copyButtonText}>Show</Text>
            </TouchableOpacity>
          </View>

          <Text style={styles.label}>Balances</Text>
          {shieldedBalances.length > 0 ? (
            shieldedBalances.map(([token, balance]) => (
              <View key={token} style={styles.balanceRow}>
                <Text style={styles.tokenName}>{getTokenDisplayName(token)}</Text>
                <Text style={styles.balanceValue}>{formatBalance(balance)}</Text>
              </View>
            ))
          ) : (
            <Text style={styles.emptyBalance}>No balance</Text>
          )}

          <Text style={styles.label}>Coins</Text>
          <Text style={styles.coinCount}>
            {state.shielded.availableCoins.length} available
            {state.shielded.pendingCoins.length > 0 && (
              <Text style={styles.pendingCount}> · {state.shielded.pendingCoins.length} pending</Text>
            )}
          </Text>

          <Text style={styles.label}>Sync Progress</Text>
          <View style={styles.progressContainer}>
            <View style={styles.progressBar}>
              <View style={[styles.progressFill, styles.shieldedIndicator, {width: `${shieldedSync.percentage}%`}]} />
            </View>
            <Text style={styles.progressText}>{formatSyncProgress(shieldedSync)}</Text>
          </View>
        </View>
      </View>

      {/* Unshielded Wallet */}
      <View style={styles.walletCard}>
        <View style={styles.walletHeader}>
          <View style={[styles.walletIndicator, styles.unshieldedIndicator]} />
          <Text style={styles.walletTitle}>Unshielded Wallet</Text>
        </View>

        <View style={styles.walletContent}>
          <Text style={styles.label}>Address</Text>
          <View style={styles.addressRow}>
            <Text style={styles.address} selectable numberOfLines={1}>
              {truncateAddress(unshieldedAddressStr)}
            </Text>
            <TouchableOpacity
              style={styles.copyButton}
              onPress={() => showAddress('Unshielded Address', unshieldedAddressStr)}
            >
              <Text style={styles.copyButtonText}>Show</Text>
            </TouchableOpacity>
          </View>

          <Text style={styles.label}>Balances</Text>
          {unshieldedBalances.length > 0 ? (
            unshieldedBalances.map(([token, balance]) => (
              <View key={token} style={styles.balanceRow}>
                <Text style={styles.tokenName}>{token === NIGHT_TOKEN_ID ? 'NIGHT' : getTokenDisplayName(token)}</Text>
                <Text style={styles.balanceValue}>{formatBalance(balance)}</Text>
              </View>
            ))
          ) : (
            <Text style={styles.emptyBalance}>No balance</Text>
          )}

          <Text style={styles.label}>Coins</Text>
          <Text style={styles.coinCount}>
            {state.unshielded.availableCoins.length} available
            {state.unshielded.pendingCoins.length > 0 && (
              <Text style={styles.pendingCount}> · {state.unshielded.pendingCoins.length} pending</Text>
            )}
          </Text>

          <Text style={styles.label}>Sync Progress</Text>
          <View style={styles.progressContainer}>
            <View style={styles.progressBar}>
              <View
                style={[styles.progressFill, styles.unshieldedIndicator, {width: `${unshieldedSync.percentage}%`}]}
              />
            </View>
            <Text style={styles.progressText}>{formatSyncProgress(unshieldedSync)}</Text>
          </View>
        </View>
      </View>

      {/* Dust Wallet */}
      <View style={styles.walletCard}>
        <View style={styles.walletHeader}>
          <View style={[styles.walletIndicator, styles.dustIndicator]} />
          <Text style={styles.walletTitle}>Dust Wallet</Text>
        </View>

        <View style={styles.walletContent}>
          <Text style={styles.label}>Address</Text>
          <View style={styles.addressRow}>
            <Text style={styles.address} selectable numberOfLines={1}>
              {truncateAddress(dustAddressStr)}
            </Text>
            <TouchableOpacity style={styles.copyButton} onPress={() => showAddress('Dust Address', dustAddressStr)}>
              <Text style={styles.copyButtonText}>Show</Text>
            </TouchableOpacity>
          </View>

          <Text style={styles.label}>Balance (DUST)</Text>
          <View style={styles.balanceRow}>
            <Text style={styles.tokenName}>DUST</Text>
            <Text style={styles.balanceValue}>{formatBalance(dustBalance)}</Text>
          </View>

          <Text style={styles.label}>Coins</Text>
          <Text style={styles.coinCount}>
            {state.dust.availableCoins.length} available
            {state.dust.pendingCoins.length > 0 && (
              <Text style={styles.pendingCount}> · {state.dust.pendingCoins.length} pending</Text>
            )}
          </Text>

          <Text style={styles.label}>Sync Progress</Text>
          <View style={styles.progressContainer}>
            <View style={styles.progressBar}>
              <View style={[styles.progressFill, styles.dustIndicator, {width: `${dustSync.percentage}%`}]} />
            </View>
            <Text style={styles.progressText}>{formatSyncProgress(dustSync)}</Text>
          </View>
        </View>
      </View>
    </ScrollView>
  );
};

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: '#0f0f1a',
  },
  content: {
    padding: 16,
    paddingBottom: 32,
  },
  statusBar: {
    flexDirection: 'row',
    alignItems: 'center',
    backgroundColor: '#1a1a2e',
    padding: 12,
    borderRadius: 8,
    marginBottom: 16,
  },
  statusDot: {
    width: 10,
    height: 10,
    borderRadius: 5,
    marginRight: 8,
  },
  statusSynced: {
    backgroundColor: '#4ade80',
  },
  statusSyncing: {
    backgroundColor: '#fbbf24',
  },
  statusText: {
    color: '#e0e0ff',
    fontSize: 14,
    fontWeight: '600',
  },
  totalBalanceCard: {
    backgroundColor: '#1a1a2e',
    borderRadius: 12,
    marginBottom: 16,
    padding: 24,
    alignItems: 'center',
    borderWidth: 1,
    borderColor: '#2a2a4a',
  },
  totalBalanceLabel: {
    color: '#8b8ba7',
    fontSize: 14,
    marginBottom: 8,
  },
  totalBalanceValue: {
    color: '#ffffff',
    fontSize: 36,
    fontWeight: '700',
    fontFamily: 'Courier',
  },
  totalBalanceCurrency: {
    color: '#a855f7',
    fontSize: 16,
    fontWeight: '600',
    marginTop: 4,
  },
  walletCard: {
    backgroundColor: '#1a1a2e',
    borderRadius: 12,
    marginBottom: 16,
    borderWidth: 1,
    borderColor: '#2a2a4a',
    overflow: 'hidden',
  },
  walletHeader: {
    flexDirection: 'row',
    alignItems: 'center',
    padding: 12,
    borderBottomWidth: 1,
    borderBottomColor: '#2a2a4a',
  },
  walletIndicator: {
    width: 4,
    height: 20,
    borderRadius: 2,
    marginRight: 10,
  },
  shieldedIndicator: {
    backgroundColor: '#a855f7', // Purple/Magenta
  },
  unshieldedIndicator: {
    backgroundColor: '#3b82f6', // Blue
  },
  dustIndicator: {
    backgroundColor: '#eab308', // Yellow
  },
  walletTitle: {
    color: '#ffffff',
    fontSize: 16,
    fontWeight: '600',
  },
  walletContent: {
    padding: 12,
  },
  label: {
    color: '#8b8ba7',
    fontSize: 12,
    marginTop: 12,
    marginBottom: 4,
  },
  addressRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  address: {
    flex: 1,
    color: '#e0e0ff',
    fontSize: 12,
    fontFamily: 'Courier',
    backgroundColor: '#12121f',
    padding: 8,
    borderRadius: 6,
    overflow: 'hidden',
  },
  copyButton: {
    backgroundColor: '#2a2a4a',
    paddingHorizontal: 12,
    paddingVertical: 8,
    borderRadius: 6,
  },
  copyButtonText: {
    color: '#a855f7',
    fontSize: 12,
    fontWeight: '600',
  },
  balanceRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    backgroundColor: '#12121f',
    padding: 10,
    borderRadius: 6,
    marginBottom: 4,
  },
  tokenName: {
    color: '#8b8ba7',
    fontSize: 14,
  },
  balanceValue: {
    color: '#ffffff',
    fontSize: 16,
    fontWeight: '700',
    fontFamily: 'Courier',
  },
  emptyBalance: {
    color: '#5a5a7a',
    fontSize: 14,
    fontStyle: 'italic',
    backgroundColor: '#12121f',
    padding: 10,
    borderRadius: 6,
  },
  coinCount: {
    color: '#e0e0ff',
    fontSize: 14,
    backgroundColor: '#12121f',
    padding: 8,
    borderRadius: 6,
  },
  pendingCount: {
    color: '#8b8ba7',
  },
  progressContainer: {
    marginTop: 4,
  },
  progressBar: {
    height: 6,
    backgroundColor: '#12121f',
    borderRadius: 3,
    overflow: 'hidden',
    marginBottom: 4,
  },
  progressFill: {
    height: '100%',
    borderRadius: 3,
  },
  progressText: {
    color: '#8b8ba7',
    fontSize: 11,
    textAlign: 'right',
  },
});
