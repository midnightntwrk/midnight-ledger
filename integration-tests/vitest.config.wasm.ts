import path from 'path';
import { defineConfig } from 'vitest/config';
import config from './vitest.config';

// eslint-disable-next-line import/no-default-export
export default defineConfig({
  ...config,
  test: {
    ...config.test,
    testTimeout: 3 * config.test!.testTimeout!,
    hookTimeout: 3 * config.test!.hookTimeout!,
    pool: 'threads',
    poolOptions: {
      threads: {
        minThreads: 4,
        maxThreads: 4
      }
    }
  }
});
