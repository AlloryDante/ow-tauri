#!/usr/bin/env node
/// <reference types="node" />
/**
 * The `ow-tauri` executable of `tauri-plugin-overwolf-cli`. Run it from the
 * local install (`npm exec --no -- ow-tauri <command>` or a package.json
 * script), never as `npx ow-tauri`, which could fetch an unrelated package.
 *
 * @packageDocumentation
 */

import { run } from './run.js';

process.exitCode = await run(process.argv.slice(2), {
  env: process.env,
  cwd: process.cwd(),
  platform: process.platform,
  log: {
    info: (message) => {
      process.stdout.write(`ow-tauri: ${message}\n`);
    },
    warn: (message) => {
      process.stderr.write(`ow-tauri: ${message}\n`);
    },
  },
  out: (text) => {
    process.stdout.write(text);
  },
});
