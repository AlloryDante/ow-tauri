#!/usr/bin/env node
// Builds the Tauri app as a debug build with its page embedded and runs it
// (`npm run start:tauri`). Arguments after `--` go to the app:
//
//   node scripts/run.mjs [--no-build] [-- <app args>]
//
// Why not `tauri dev`: under `tauri dev` the page comes from the Tauri CLI's
// dev server, which stops when the app's first process exits, so an in-app
// restart (Restart in TEST/LIVE) would come back to a window with no page.
// A built app carries its page and restarts like a shipped one. Use
// `npm run dev:tauri` for hot reload instead.
//
// In an example with scripts/stage.mjs the page is staged first and the
// staged identity (.stage/tauri.conf.json) is passed as `--config`. The
// binary is src-tauri/target/debug/<name> (CARGO_TARGET_DIR is honoured;
// `.exe` on Windows). The same file is in every example (a test checks it).

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const argv = process.argv.slice(2);
const split = argv.indexOf('--');
const own = split === -1 ? argv : argv.slice(0, split);
const appArgs = split === -1 ? [] : argv.slice(split + 1);
const noBuild = own.includes('--no-build');

/**
 * The binary name of the app: the `[[bin]]` name, else the package name.
 *
 * @param {string} manifest Cargo.toml text
 * @returns {string}
 */
export function binaryName(manifest) {
  const bin = /\[\[bin\]\][^[]*?\bname\s*=\s*"([^"]+)"/s.exec(manifest);
  if (bin) return bin[1];
  const pkg = /\[package\][^[]*?\bname\s*=\s*"([^"]+)"/s.exec(manifest);
  if (!pkg) throw new Error('src-tauri/Cargo.toml has no package name');
  return pkg[1];
}

function run(cmd, args) {
  const r = spawnSync(cmd, args, { cwd: root, stdio: 'inherit' });
  if (r.status !== 0) process.exit(r.status ?? 1);
}

function main() {
  const manifest = readFileSync(join(root, 'src-tauri', 'Cargo.toml'), 'utf8');
  const targetDir = process.env.CARGO_TARGET_DIR
    ? resolve(process.env.CARGO_TARGET_DIR)
    : join(root, 'src-tauri', 'target');
  const exe = join(
    targetDir,
    'debug',
    binaryName(manifest) + (process.platform === 'win32' ? '.exe' : ''),
  );
  if (!noBuild || !existsSync(exe)) {
    const config = [];
    const stage = join(root, 'scripts', 'stage.mjs');
    if (existsSync(stage)) {
      // Stages the page and writes .stage/tauri.conf.json (the identity).
      run(process.execPath, [stage, '--host', 'tauri']);
      config.push('--config', join(root, '.stage', 'tauri.conf.json'));
    }
    const cli = createRequire(import.meta.url).resolve('@tauri-apps/cli/tauri.js');
    run(process.execPath, [cli, 'build', '--debug', '--no-bundle', ...config]);
  }
  const child = spawn(exe, appArgs, { cwd: root, stdio: 'inherit' });
  child.on('exit', (code, signal) => {
    process.exit(signal ? 1 : (code ?? 0));
  });
  for (const signal of ['SIGINT', 'SIGTERM']) {
    process.on(signal, () => {
      child.kill(signal);
    });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
