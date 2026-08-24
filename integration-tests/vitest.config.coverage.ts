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
      '@midnightntwrk/ledger': path.resolve(
        __dirname,
        'lib-sources/@midnightntwrk/ledger-v10/midnight_ledger_wasm_v10.js'
      )
    }
  },
  test: {
    ...config.test,
    coverage: {
      ...config.test?.coverage,
      enabled: true,
      reportOnFailure: true
    }
  },
  plugins: [wasm()]
});
