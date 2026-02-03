// This file is part of lunar-spark.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import React, {useState, useEffect, useCallback} from 'react';
import {
  StyleSheet,
  Text,
  View,
  TouchableOpacity,
  TextInput,
  SafeAreaView,
  Alert,
  ActivityIndicator,
  KeyboardAvoidingView,
  Platform,
} from 'react-native';
import {StatusBar} from 'expo-status-bar';
import type {Subscription} from 'rxjs';
import type {FacadeState} from '@midnight-ntwrk/wallet-sdk-facade';
import {initializeWallet, stopWallet, type WalletInitResult} from './src/lib/wallet';
import {type Environment, getEnvironmentConfig, ENVIRONMENT_OPTIONS} from './src/config/environments';
import {WalletDashboard} from './src/components/WalletDashboard';

type AppState = 'setup' | 'initializing' | 'connected' | 'error';

export default function App() {
  // App state
  const [appState, setAppState] = useState<AppState>('setup');
  const [error, setError] = useState<string | null>(null);

  // Environment selection
  const [selectedEnv, setSelectedEnv] = useState<Environment>('preprod');

  // Seed input (hex string for now)
  const [seedHex, setSeedHex] = useState('');

  // Wallet state
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
        setAppState('error');
      },
    });

    return () => {
      subscription.unsubscribe();
    };
  }, [walletResult?.facade]);

  // Initialize wallet with seed
  const handleConnect = useCallback(async () => {
    if (!seedHex || seedHex.length !== 64) {
      Alert.alert('Invalid Seed', 'Please enter a valid 32-byte seed as 64 hex characters');
      return;
    }

    setAppState('initializing');
    setError(null);

    try {
      // Convert hex to Uint8Array
      const seed = new Uint8Array(32);
      for (let i = 0; i < 32; i++) {
        seed[i] = parseInt(seedHex.substring(i * 2, i * 2 + 2), 16);
      }

      const envConfig = getEnvironmentConfig(selectedEnv);
      const result = await initializeWallet(seed, envConfig);

      setWalletResult(result);
      setAppState('connected');
    } catch (err) {
      console.log(err);
      const message = err instanceof Error ? err.message : String(err);
      setError(`Failed to initialize wallet: ${message}`);
      setAppState('error');
    }
  }, [seedHex, selectedEnv]);

  // Disconnect wallet
  const handleDisconnect = useCallback(async () => {
    if (walletResult) {
      try {
        await stopWallet(walletResult);
      } catch (err) {
        console.warn('Error stopping wallet:', err);
      }
    }
    setWalletResult(null);
    setFacadeState(null);
    setAppState('setup');
    setError(null);
  }, [walletResult]);

  // Render based on app state
  if (appState === 'connected' && facadeState && walletResult) {
    return (
      <SafeAreaView style={styles.container}>
        <StatusBar style="light" />
        <View style={styles.header}>
          <View>
            <Text style={styles.title}>Lunar Spark</Text>
            <Text style={styles.envLabel}>{selectedEnv.toUpperCase()}</Text>
          </View>
          <TouchableOpacity style={styles.disconnectButton} onPress={handleDisconnect}>
            <Text style={styles.disconnectText}>Disconnect</Text>
          </TouchableOpacity>
        </View>
        <WalletDashboard state={facadeState} networkId={walletResult.networkId} />
      </SafeAreaView>
    );
  }

  if (appState === 'initializing') {
    return (
      <SafeAreaView style={styles.container}>
        <StatusBar style="light" />
        <View style={styles.centerContent}>
          <ActivityIndicator size="large" color="#7b68ee" />
          <Text style={styles.loadingText}>Initializing wallet...</Text>
          <Text style={styles.loadingSubtext}>Connecting to {selectedEnv}</Text>
        </View>
      </SafeAreaView>
    );
  }

  // Setup screen
  return (
    <SafeAreaView style={styles.container}>
      <StatusBar style="light" />
      <KeyboardAvoidingView style={styles.setupContainer} behavior={Platform.OS === 'ios' ? 'padding' : 'height'}>
        <Text style={styles.title}>Lunar Spark</Text>
        <Text style={styles.subtitle}>Midnight Wallet</Text>

        {/* Environment Selection */}
        <View style={styles.section}>
          <Text style={styles.sectionTitle}>Network</Text>
          <View style={styles.envButtonRow}>
            {ENVIRONMENT_OPTIONS.map((opt) => (
              <TouchableOpacity
                key={opt.value}
                style={[styles.envButton, selectedEnv === opt.value && styles.envButtonSelected]}
                onPress={() => setSelectedEnv(opt.value)}
              >
                <Text style={[styles.envButtonText, selectedEnv === opt.value && styles.envButtonTextSelected]}>
                  {opt.label}
                </Text>
              </TouchableOpacity>
            ))}
          </View>
        </View>

        {/* Seed Input */}
        <View style={styles.section}>
          <Text style={styles.sectionTitle}>Wallet Seed (Hex)</Text>
          <TextInput
            style={styles.seedInput}
            placeholder="Enter 64 character hex seed..."
            placeholderTextColor="#5a5a7a"
            value={seedHex}
            onChangeText={setSeedHex}
            autoCapitalize="none"
            autoCorrect={false}
            multiline
          />
          <Text style={styles.seedHint}>
            {seedHex.length}/64 characters
            {seedHex.length === 64 && ' ✓'}
          </Text>
        </View>

        {/* Error Display */}
        {error && (
          <View style={styles.errorContainer}>
            <Text style={styles.errorText}>{error}</Text>
          </View>
        )}

        {/* Connect Button */}
        <TouchableOpacity
          style={[styles.connectButton, seedHex.length !== 64 && styles.connectButtonDisabled]}
          onPress={handleConnect}
          disabled={seedHex.length !== 64}
        >
          <Text style={styles.connectButtonText}>Connect Wallet</Text>
        </TouchableOpacity>
      </KeyboardAvoidingView>
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: '#0f0f1a',
  },
  header: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    padding: 16,
    borderBottomWidth: 1,
    borderBottomColor: '#2a2a4a',
  },
  title: {
    fontSize: 24,
    fontWeight: 'bold',
    color: '#ffffff',
  },
  envLabel: {
    fontSize: 12,
    color: '#7b68ee',
    fontWeight: '600',
  },
  disconnectButton: {
    backgroundColor: '#3a3a5a',
    paddingVertical: 8,
    paddingHorizontal: 16,
    borderRadius: 6,
  },
  disconnectText: {
    color: '#e0e0ff',
    fontSize: 14,
    fontWeight: '600',
  },
  centerContent: {
    flex: 1,
    justifyContent: 'center',
    alignItems: 'center',
    padding: 20,
  },
  loadingText: {
    color: '#ffffff',
    fontSize: 18,
    fontWeight: '600',
    marginTop: 20,
  },
  loadingSubtext: {
    color: '#8b8ba7',
    fontSize: 14,
    marginTop: 8,
  },
  setupContainer: {
    flex: 1,
    padding: 20,
    justifyContent: 'center',
  },
  subtitle: {
    fontSize: 16,
    color: '#8b8ba7',
    textAlign: 'center',
    marginBottom: 40,
  },
  section: {
    marginBottom: 24,
  },
  sectionTitle: {
    fontSize: 14,
    fontWeight: '600',
    color: '#7b68ee',
    marginBottom: 12,
  },
  envButtonRow: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    gap: 8,
  },
  envButton: {
    backgroundColor: '#1a1a2e',
    paddingVertical: 10,
    paddingHorizontal: 16,
    borderRadius: 8,
    borderWidth: 1,
    borderColor: '#2a2a4a',
  },
  envButtonSelected: {
    backgroundColor: '#4a3f8a',
    borderColor: '#7b68ee',
  },
  envButtonText: {
    color: '#8b8ba7',
    fontSize: 13,
    fontWeight: '500',
  },
  envButtonTextSelected: {
    color: '#ffffff',
  },
  seedInput: {
    backgroundColor: '#1a1a2e',
    borderWidth: 1,
    borderColor: '#2a2a4a',
    borderRadius: 8,
    padding: 12,
    color: '#e0e0ff',
    fontFamily: 'Courier',
    fontSize: 12,
    minHeight: 80,
    textAlignVertical: 'top',
  },
  seedHint: {
    color: '#5a5a7a',
    fontSize: 12,
    marginTop: 8,
    textAlign: 'right',
  },
  errorContainer: {
    backgroundColor: '#2e1a1a',
    borderWidth: 1,
    borderColor: '#5a2a2a',
    borderRadius: 8,
    padding: 12,
    marginBottom: 16,
  },
  errorText: {
    color: '#ff6b6b',
    fontSize: 13,
  },
  connectButton: {
    backgroundColor: '#4a3f8a',
    paddingVertical: 16,
    borderRadius: 10,
    alignItems: 'center',
  },
  connectButtonDisabled: {
    backgroundColor: '#2a2a4a',
    opacity: 0.6,
  },
  connectButtonText: {
    color: '#ffffff',
    fontSize: 16,
    fontWeight: '700',
  },
});
