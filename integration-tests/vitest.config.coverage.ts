import path from 'path';
import { defineConfig } from 'vitest/config';
import wasm from 'vite-plugin-wasm';
import config from './vitest.config';

// eslint-disable-next-line import/no-default-export
export default defineConfig({
  ...config,
  resolve: {
    alias: {
      ...config.resolve?.alias,
      '@midnight-ntwrk/ledger': path.resolve(__dirname, 'lib-sources/@midnight-ntwrk/ledger-v6/midnight_ledger_wasm.js')
    }
  },
  test: {
    ...config.test,
    coverage: {
      ...config.test?.coverage,
      enabled: true
    }
  },
  plugins: [wasm()]
});
