import { defineConfig } from 'vitest/config';
import path from 'path';

// eslint-disable-next-line import/no-default-export
export default defineConfig({
  test: {
    globals: true,
    name: 'Ledger API',
    pool: 'threads',
    environment: 'node',
    dir: './src/test',
    include: ['**/*.test.ts'],
    setupFiles: ['src/vitest.setup.ts'],
    testTimeout: 15 * 60_000,
    hookTimeout: 15 * 60_000,
    reporters: [
      'default',
      ['junit', { outputFile: './reports/test-report.xml' }],
      ['html', { outputFile: './reports/html/index.html' }],
      ['@d2t/vitest-ctrf-json-reporter', { outputDir: './reports/', outputFile: 'ctrf-report.json' }],
      ['allure-vitest/reporter', { resultsDir: './reports/allure-results' }]
    ],
    coverage: {
      include: ['lib-sources/@midnight-ntwrk/ledger/**/*.js'],
      provider: 'v8',
      reporter: ['clover', 'json', 'json-summary', 'lcov', 'text'],
      reportsDirectory: './coverage',
      enabled: false
    }
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src')
    }
  }
});
