import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    // Renderer code needs a DOM; @tauri-apps/api/mocks replaces the Tauri IPC.
    environment: 'happy-dom',
    // The reference dev machine is a fanless laptop: never more than 2 workers.
    maxWorkers: 2,
    restoreMocks: true,
    coverage: {
      provider: 'v8',
      include: ['src/**/*.ts'],
      exclude: [
        'src/**/*.test.ts',
        'src/**/*.d.ts',
        'src/bootstrap/index.ts',
        'src/bootstrap/transport.ts',
      ],
      reporter: ['text-summary', 'text'],
      // Core modules (codec, IPC, state cache, kernel) must stay at 90 % lines.
      thresholds: {
        'src/shared/**': { lines: 90 },
        'src/bootstrap/**': { lines: 90 },
        'src/electron/**': { lines: 80 },
      },
    },
  },
});
