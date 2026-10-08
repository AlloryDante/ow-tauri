// Builds `crates/tauri-plugin-overwolf/api-iife.js`: the main entry (`.`) as
// one IIFE for apps with `app.withGlobalTauri` (`window.__TAURI__.overwolf`),
// like the official plugins' `api-iife.js`. It never contains the
// `<owadview>` runtime, uses `window.__TAURI__.core` instead of bundling
// `@tauri-apps/api`, and does nothing in the plugin's own webviews (ad
// guests `owad-*`, consent windows `ow-cmp*`), so ad pages never see it.
//
// The output is committed (cargo publish and docs.rs need no npm step);
// `npm run check:generated` at the root fails when it is stale.
// OW_TAURI_GENERATED_OUT_DIR overrides the crate folder (the drift check).
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { defineConfig } from 'rolldown';

const root = dirname(fileURLToPath(import.meta.url));
const outDir =
  process.env.OW_TAURI_GENERATED_OUT_DIR ?? join(root, '../../crates/tauri-plugin-overwolf');

const RESERVED =
  'var l=window.__TAURI_INTERNALS__&&window.__TAURI_INTERNALS__.metadata&&window.__TAURI_INTERNALS__.metadata.currentWebview&&window.__TAURI_INTERNALS__.metadata.currentWebview.label;';

export default defineConfig({
  input: join(root, 'src/index.ts'),
  platform: 'browser',
  external: ['@tauri-apps/api/core'],
  output: {
    file: join(outDir, 'api-iife.js'),
    format: 'iife',
    name: '__TAURI_PLUGIN_OVERWOLF__',
    globals: { '@tauri-apps/api/core': 'window.__TAURI__.core' },
    banner: `if("__TAURI__"in window&&!function(){${RESERVED}return typeof l=="string"&&(l.indexOf("owad-")===0||l.indexOf("ow-cmp")===0)}()){`,
    footer: 'Object.defineProperty(window.__TAURI__,"overwolf",{value:__TAURI_PLUGIN_OVERWOLF__})}',
    minify: true,
    sourcemap: false,
  },
});
