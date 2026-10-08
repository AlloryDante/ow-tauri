import { readFileSync } from 'node:fs';

import { defineConfig } from 'vitest/config';

const { version } = JSON.parse(readFileSync(new URL('package.json', import.meta.url), 'utf8')) as {
  version: string;
};

export default defineConfig({
  // The RUNTIME_VERSION test compares against it.
  define: { __PACKAGE_VERSION__: JSON.stringify(version) },
  test: {
    include: ['src/**/*.test.ts'],
    // The element runtime needs a DOM; @tauri-apps/api/mocks replaces the Tauri IPC.
    environment: 'happy-dom',
    // The reference dev machine is a fanless laptop: never more than 2 workers.
    maxWorkers: 2,
    restoreMocks: true,
    coverage: {
      provider: 'v8',
      include: ['src/**/*.ts'],
      exclude: ['src/**/*.test.ts', 'src/jsx.ts'],
      reporter: ['text-summary', 'text'],
      thresholds: { lines: 90, branches: 85, functions: 90 },
    },
  },
});
