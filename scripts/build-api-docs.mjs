#!/usr/bin/env node
// Builds the API reference (docs/api/README.md): typedoc for the npm package
// and rustdoc for the plugin crate. The output is generated and git-ignored;
// never commit it.
//
//   packages/ow-tauri/docs-out/index.html        ow-tauri/main, /electron, /renderer, /testing
//   target/doc/tauri_plugin_overwolf/index.html  the Rust plugin
//
// Usage: node scripts/build-api-docs.mjs [--ts] [--rust]
// With neither flag, both are built. Warnings fail the build in both tools,
// as in CI. CARGO_BUILD_JOBS limits cargo's parallel jobs.

import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const args = new Set(process.argv.slice(2));
for (const arg of args) {
  if (arg !== '--ts' && arg !== '--rust') {
    console.error(`unknown option ${arg}\nUsage: node scripts/build-api-docs.mjs [--ts] [--rust]`);
    process.exit(2);
  }
}
const both = args.size === 0;

/**
 * Runs a command in the repository root and exits on failure.
 *
 * @param {string} command
 * @param {string[]} commandArgs
 * @param {NodeJS.ProcessEnv} [env]
 */
function run(command, commandArgs, env = process.env) {
  console.log(`> ${command} ${commandArgs.join(' ')}`);
  const result = spawnSync(command, commandArgs, {
    cwd: root,
    env,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  });
  if (result.error) {
    console.error(result.error.message);
    process.exit(1);
  }
  if (result.status !== 0) process.exit(result.status ?? 1);
}

const outputs = [];

if (both || args.has('--ts')) {
  run('npm', ['run', 'docs', '--workspace', 'ow-tauri']);
  outputs.push(join(root, 'packages', 'ow-tauri', 'docs-out', 'index.html'));
}

if (both || args.has('--rust')) {
  run('cargo', ['doc', '--no-deps', '--all-features', '--locked', '-p', 'tauri-plugin-overwolf'], {
    ...process.env,
    RUSTDOCFLAGS: [process.env.RUSTDOCFLAGS, '-D warnings'].filter(Boolean).join(' '),
  });
  outputs.push(join(root, 'target', 'doc', 'tauri_plugin_overwolf', 'index.html'));
}

for (const output of outputs) {
  if (!existsSync(output)) {
    console.error(`expected ${output}, but it was not written`);
    process.exit(1);
  }
  console.log(`API reference: ${output}`);
}
