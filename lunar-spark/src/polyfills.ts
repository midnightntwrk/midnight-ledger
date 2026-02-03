// This file is part of lunar-spark.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

/**
 * Polyfills for React Native environment.
 * This file MUST be imported before any other code.
 */

// Buffer polyfill - many crypto/blockchain libraries expect a global Buffer
import { Buffer } from '@craftzdog/react-native-buffer';

// Set up global Buffer if not present
if (typeof globalThis.Buffer === 'undefined') {
  (globalThis as any).Buffer = Buffer;
}

// Also set on global for older code patterns
if (typeof global !== 'undefined' && typeof (global as any).Buffer === 'undefined') {
  (global as any).Buffer = Buffer;
}

console.log('[polyfills] Buffer polyfill installed');
