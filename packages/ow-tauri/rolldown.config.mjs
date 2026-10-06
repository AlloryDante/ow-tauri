// Builds the scripts the plugin injects (CONTRACT section B, "One runtime
// per webview"; ADR 0003 for the ad guests), each one self-contained IIFE:
//
//   src/bootstrap/index.ts  -> js/bootstrap.js    (every app webview)
//   src/guest/adview-host.ts -> js/adview-host.js (ad guest webviews)
//   src/guest/cmp.ts        -> js/cmp.js          (the consent window)
//
// `js/` is crates/tauri-plugin-overwolf/js; OW_TAURI_INJECTED_OUT_DIR
// overrides it (the drift check builds into a temporary directory). An
// entry whose source does not exist yet is skipped. The output is committed,
// so `cargo build` works without Node; `npm run check:injected` fails when
// it is stale.
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { defineConfig } from 'rolldown';

const root = dirname(fileURLToPath(import.meta.url));
const outDir =
  process.env.OW_TAURI_INJECTED_OUT_DIR ?? join(root, '../../crates/tauri-plugin-overwolf/js');

/** Injected scripts: source (relative to this package) and output file name. */
export const INJECTED = [
  { input: 'src/bootstrap/index.ts', file: 'bootstrap.js' },
  {
    input: 'src/guest/adview-host.ts',
    file: 'adview-host.js',
    token: '__OW_TAURI_ADVIEW_CONFIG__',
  },
  { input: 'src/guest/cmp.ts', file: 'cmp.js', token: '__OW_TAURI_CMP_CONFIG__' },
];

// The guest entries read their configuration from a free identifier, which
// the minifier leaves alone; after minification it becomes the comment token
// Rust replaces (CONTRACT D.1). The build fails unless it occurs exactly once.
const configToken = (token) => ({
  name: 'ow-tauri-config-token',
  generateBundle(_options, bundle) {
    if (token === undefined) return;
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
  INJECTED.filter((entry) => existsSync(join(root, entry.input))).map((entry) => ({
    input: join(root, entry.input),
    platform: 'browser',
    plugins: [configToken(entry.token)],
    output: {
      file: join(outDir, entry.file),
      format: 'iife',
      minify: true,
      sourcemap: false,
    },
  })),
);
