// Builds the Tauri edition of the parity harness app: the web assets into
// dist/ (web/harness.js bundled with the workspace's rolldown, the plugin's
// JavaScript API inlined), then the Rust app in debug (the plugin's `lab`
// feature). `run.mjs --host tauri` calls buildTauriApp() once per run.
//
//   node tauri-app/build.mjs          # build and print the binary path

import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { SCENARIOS } from '../lib/scenarios.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, '..', '..', '..');
const distDir = join(here, 'dist');
const crateDir = join(here, 'src-tauri');

/**
 * The app files the scenarios' `open-window` actions load (Tauri serves only
 * what the build embeds; ow-electron's harness writes them on demand).
 * @returns {string[]}
 */
export function extraWindowFiles() {
  const files = new Set(['blank.html']);
  for (const scenario of Object.values(SCENARIOS)) {
    for (const action of scenario.config?.actions ?? []) {
      if (action.do === 'open-window' && !action.url) files.add(action.file ?? 'blank.html');
    }
  }
  files.delete('index.html');
  return [...files].sort();
}

/** The page an extra window shows (as ow-electron's harness writes it). */
export function extraWindowPage(file) {
  return `<!doctype html><meta charset="utf-8"><title>page ${basename(file)}</title><body style="background:#202020"></body>\n`;
}

/** Bundles web/harness.js and copies the pages. */
export async function buildWeb() {
  const require = createRequire(join(repoRoot, 'package.json'));
  // A file URL: on Windows a bare absolute path reads as a URL scheme (`d:`).
  const { build } = await import(pathToFileURL(require.resolve('rolldown')).href);
  rmSync(distDir, { recursive: true, force: true });
  mkdirSync(distDir, { recursive: true });
  await build({
    input: join(here, 'web', 'harness.js'),
    platform: 'browser',
    resolve: { modules: [join(repoRoot, 'node_modules'), 'node_modules'] },
    output: { file: join(distDir, 'harness.js'), format: 'iife' },
    logLevel: 'warn',
  });
  for (const file of ['index.html', 'page-shim.js']) {
    copyFileSync(join(here, 'web', file), join(distDir, file));
  }
  // The harness page itself is shared with the ow-electron app.
  copyFileSync(join(here, '..', 'app', 'page.js'), join(distDir, 'page.js'));
  for (const file of extraWindowFiles()) {
    writeFileSync(join(distDir, file), extraWindowPage(file));
  }
}

/**
 * Builds the web assets and the debug binary (`cargo build -j 4`, at
 * background priority on macOS) and returns the binary path.
 * @returns {Promise<string>}
 */
export async function buildTauriApp() {
  await buildWeb();
  const cargo = ['cargo', 'build', '-j', '4', '--manifest-path', join(crateDir, 'Cargo.toml')];
  const [cmd, ...args] = process.platform === 'darwin' ? ['taskpolicy', '-b', ...cargo] : cargo;
  const result = spawnSync(cmd, args, { stdio: ['ignore', 'inherit', 'inherit'] });
  if (result.status !== 0) throw new Error(`cargo build failed (${result.status})`);
  const exe = join(
    crateDir,
    'target',
    'debug',
    `ow-tauri-parity-harness${process.platform === 'win32' ? '.exe' : ''}`,
  );
  if (!existsSync(exe)) throw new Error(`missing ${exe}`);
  return exe;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  buildTauriApp().then(
    (exe) => console.log(exe),
    (error) => {
      console.error(error);
      process.exit(1);
    },
  );
}
