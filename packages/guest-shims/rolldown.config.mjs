// Builds the scripts the plugin injects into its own webviews, each one
// self-contained IIFE (DESIGN: guest shims, committed like api-iife.js):
//
//   src/adview-host.ts     -> js/adview-host.js     (ad guests owad-*)
//   src/cmp.ts             -> js/cmp.js             (consent windows ow-cmp*)
//   src/session-restore.ts -> js/session-restore.js (one-shot recreate prelude, macOS)
//
// `js/` is crates/tauri-plugin-overwolf/js; OW_TAURI_GENERATED_OUT_DIR
// overrides the crate folder (the drift check builds into a temporary
// directory). The output is committed, so `cargo build` works without Node;
// `npm run check:generated` at the root fails when it is stale.
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { defineConfig } from 'rolldown';

const root = dirname(fileURLToPath(import.meta.url));
const crate =
  process.env.OW_TAURI_GENERATED_OUT_DIR ?? join(root, '../../crates/tauri-plugin-overwolf');

/** Injected scripts: source, output file name, and the configuration token Rust replaces. */
export const INJECTED = [
  { input: 'src/adview-host.ts', file: 'adview-host.js', token: '__OW_TAURI_ADVIEW_CONFIG__' },
  { input: 'src/cmp.ts', file: 'cmp.js', token: '__OW_TAURI_CMP_CONFIG__' },
  {
    input: 'src/session-restore.ts',
    file: 'session-restore.js',
    token: '__OW_TAURI_SESSION_SNAPSHOT__',
  },
];

// An entry reads its configuration from a free identifier, which the
// minifier leaves alone; after minification it becomes the comment token
// Rust replaces. The build fails unless it occurs exactly once.
const configToken = (token) => ({
  name: 'ow-tauri-config-token',
  generateBundle(_options, bundle) {
    for (const chunk of Object.values(bundle)) {
      if (chunk.type !== 'chunk') continue;
      const parts = chunk.code.split(new RegExp(`\\b${token}\\b`));
      if (parts.length !== 2) {
        this.error(`${chunk.fileName}: expected ${token} exactly once, found ${parts.length - 1}`);
      }
      chunk.code = parts.join(`/*${token}*/null`);
    }
  },
});

export default defineConfig(
  INJECTED.map((entry) => ({
    input: join(root, entry.input),
    platform: 'browser',
    plugins: [configToken(entry.token)],
    output: {
      file: join(crate, 'js', entry.file),
      format: 'iife',
      minify: true,
      sourcemap: false,
    },
  })),
);
