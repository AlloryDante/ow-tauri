import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'happy-dom',
    // The reference dev machine is a fanless laptop: never more than 2 workers.
    maxWorkers: 2,
    restoreMocks: true,
    coverage: {
      provider: 'v8',
      include: ['src/**/*.ts'],
      // The entries only call the core functions with the spliced configuration.
      exclude: ['src/**/*.test.ts', 'src/adview-host.ts', 'src/cmp.ts', 'src/session-restore.ts'],
      reporter: ['text-summary', 'text'],
      thresholds: { lines: 90 },
    },
  },
});
