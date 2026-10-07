// One bundler config, two hosts. The renderer bundle is the same for both;
// the main process and the preload differ only in how `electron` resolves:
//
//   ow-electron: `electron` stays external (the real module); `#host` is
//                src/main/host/electron.ts (Node fs).
//   ow-tauri:    `electron` -> `ow-tauri/electron`; `#host` is
//                src/main/host/tauri.ts (ow-tauri/main `files`). The main
//                process becomes a classic script for the hidden main webview.
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
 * @param {{ outDir: string, mainEntry?: string }} options outDir: the staged
 *   app folder; mainEntry: replaces src/main/main.ts (the lab driver entry)
 * @returns {import('rolldown').RolldownOptions[]}
 */
export function bundles(host, { outDir, mainEntry }) {
  const electron = host === 'electron';
  const alias = {
    '#host': src(`main/host/${host}.ts`),
    ...(electron ? {} : { electron: 'ow-tauri/electron' }),
  };
  const external = electron ? ['electron'] : [];
  // tsconfig: false: tsconfig.json maps `#host` to the type-only contract for
  // `tsc`; the bundle must take the alias above instead.
  const common = { resolve: { alias }, external, tsconfig: false, logLevel: 'warn' };
  return [
    {
      ...common,
      input: mainEntry ?? src('main/main.ts'),
      platform: electron ? 'node' : 'browser',
      output: { file: join(outDir, 'main', 'main.js'), format: electron ? 'cjs' : 'iife' },
    },
    {
      ...common,
      input: src('preload/preload.ts'),
      platform: 'browser',
      output: { file: join(outDir, 'preload', 'preload.js'), format: electron ? 'cjs' : 'iife' },
    },
    {
      input: src('renderer/index.ts'),
      platform: 'browser',
      tsconfig: false,
      logLevel: 'warn',
      output: { file: join(outDir, 'renderer', 'index.js'), format: 'iife' },
    },
  ];
}

export default defineConfig([
  ...bundles('electron', { outDir: join(root, '.stage', 'electron') }),
  ...bundles('tauri', { outDir: join(root, '.stage', 'tauri') }),
]);
