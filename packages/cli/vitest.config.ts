import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
    // The reference dev machine is a fanless laptop: never more than 2 workers.
    maxWorkers: 2,
    restoreMocks: true,
    coverage: {
      provider: 'v8',
      include: ['src/**/*.ts'],
      // The executable only wires process I/O into run().
      exclude: ['src/**/*.test.ts', 'src/index.ts'],
      reporter: ['text-summary', 'text'],
      thresholds: { lines: 90, branches: 80, functions: 90 },
    },
  },
});
