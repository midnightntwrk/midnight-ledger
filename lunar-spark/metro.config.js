const { getDefaultConfig } = require('expo/metro-config');
const { withNativeWind } = require('nativewind/metro');
const path = require('path');

const projectRoot = __dirname;
const workspaceRoot = path.resolve(projectRoot, '..');
const expoLedgerPath = path.resolve(workspaceRoot, 'expo-midnight-ledger');

const config = getDefaultConfig(projectRoot);

// Watch the expo-midnight-ledger folder for changes (hot reload)
config.watchFolders = [expoLedgerPath];

config.resolver = {
  ...config.resolver,
  // Tell Metro where to find node_modules for watched folders
  nodeModulesPaths: [path.resolve(projectRoot, 'node_modules')],
  // Enable package exports to handle ESM modules properly
  unstable_enablePackageExports: true,
  // Prefer require (CommonJS) over import (ESM) to avoid frozen default issues
  unstable_conditionNames: ['require', 'react-native', 'default'],
};

module.exports = withNativeWind(config, { input: './global.css' });
