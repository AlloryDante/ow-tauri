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
  { input: 'src/guest/adview-host.ts', file: 'adview-host.js' },
  { input: 'src/guest/cmp.ts', file: 'cmp.js' },
];

export default defineConfig(
  INJECTED.filter((entry) => existsSync(join(root, entry.input))).map((entry) => ({
    input: join(root, entry.input),
    platform: 'browser',
    output: {
      file: join(outDir, entry.file),
      format: 'iife',
      minify: true,
      sourcemap: false,
    },
  })),
);
