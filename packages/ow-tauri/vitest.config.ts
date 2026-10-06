import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    // Renderer code needs a DOM; @tauri-apps/api/mocks replaces the Tauri IPC.
    environment: 'happy-dom',
    // The reference dev machine is a fanless laptop: never more than 2 workers.
    maxWorkers: 2,
    restoreMocks: true,
  },
});
