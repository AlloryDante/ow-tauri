import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

const host = process.env['TAURI_DEV_HOST'];

/**
 * Under Vitest, inside the ow-tauri repository, the API package resolves to
 * its TypeScript source, so the tests run without building the package
 * first. Builds always use the package as published (its `dist` and its
 * `sideEffects` list, which keeps `import 'tauri-plugin-overwolf-api/adview'`
 * in the bundle), exactly as in an app of your own.
 */
const apiSource = fileURLToPath(new URL('../../packages/api/src/', import.meta.url));
const API_ENTRIES: Record<string, string> = {
  '': 'index.ts',
  '/adview': 'adview/index.ts',
  '/updater': 'updater.ts',
  '/testing': 'testing/index.ts',
  '/jsx': 'jsx.ts',
};
const alias =
  process.env['VITEST'] !== undefined && existsSync(`${apiSource}index.ts`)
    ? Object.entries(API_ENTRIES).map(([entry, file]) => ({
        find: new RegExp(`^tauri-plugin-overwolf-api${entry}$`),
        replacement: `${apiSource}${file}`,
      }))
    : [];

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  resolve: { alias },
  // Keep Rust errors of `tauri dev` on screen.
  clearScreen: false,
  // `tauri dev` loads http://localhost:1420 (`build.devUrl`).
  server: {
    port: 1420,
    strictPort: true,
    host: host ?? false,
    ...(host ? { hmr: { protocol: 'ws', host, port: 1421 } } : {}),
    watch: { ignored: ['**/src-tauri/**', '**/e2e/out/**'] },
  },
  build: {
    // The WebView2 and WebKit versions Tauri 2 supports.
    target: ['es2022', 'safari16'],
  },
  test: {
    include: ['src/**/*.test.{ts,tsx}', 'scripts/**/*.test.mjs', 'e2e/**/*.test.mjs'],
    // The <owadview> runtime and React need a DOM; @tauri-apps/api/mocks
    // replaces the Tauri IPC (tauri-plugin-overwolf-api/testing).
    environment: 'happy-dom',
    restoreMocks: true,
  },
});
