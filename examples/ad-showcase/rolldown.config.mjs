// One bundler config, two hosts, the same showcase page.
//
//   ow-electron: the main process (src/main, Node + `electron`, external),
//                the preload (src/preload) and the page (src/renderer).
//   ow-tauri:    the page only: src/tauri/index.ts installs `window.showcase`
//                (tauri-plugin-overwolf-api + the app's commands) and the
//                `<owadview>` runtime, then starts the same src/renderer
//                code. The native side is src-tauri (Rust).
//
// scripts/stage.mjs builds these into .stage/<host>/ (git-ignored).
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { defineConfig } from 'rolldown';

const root = dirname(fileURLToPath(import.meta.url));
const src = (p) => join(root, 'src', p);

/**
 * The bundles of one host.
 *
 * @param {'electron' | 'tauri'} host
 * @param {{ outDir: string, entry?: string }} options outDir: the staged
 *   app folder; entry: replaces the ow-tauri page entry (the lab driver
 *   entry of `stage.mjs --lab`)
 * @returns {import('rolldown').RolldownOptions[]}
 */
export function bundles(host, { outDir, entry }) {
  const common = { tsconfig: false, logLevel: 'warn' };
  if (host === 'tauri') {
    return [
      {
        ...common,
        input: entry ?? src('tauri/index.ts'),
        platform: 'browser',
        output: { file: join(outDir, 'renderer', 'index.js'), format: 'iife' },
      },
    ];
  }
  return [
    {
      ...common,
      input: src('main/main.ts'),
      platform: 'node',
      resolve: { alias: { '#host': src('main/host/electron.ts') } },
      external: ['electron'],
      output: { file: join(outDir, 'main', 'main.js'), format: 'cjs' },
    },
    {
      ...common,
      input: src('preload/preload.ts'),
      platform: 'browser',
      external: ['electron'],
      output: { file: join(outDir, 'preload', 'preload.js'), format: 'cjs' },
    },
    {
      ...common,
      input: src('renderer/index.ts'),
      platform: 'browser',
      output: { file: join(outDir, 'renderer', 'index.js'), format: 'iife' },
    },
  ];
}

export default defineConfig([
  ...bundles('electron', { outDir: join(root, '.stage', 'electron') }),
  ...bundles('tauri', { outDir: join(root, '.stage', 'tauri') }),
]);
