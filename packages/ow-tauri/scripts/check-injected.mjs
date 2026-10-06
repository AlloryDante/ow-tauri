// Drift check for the committed injected scripts
// (crates/tauri-plugin-overwolf/js): rebuilds them into a temporary
// directory and fails when any differs from the committed file, or when a
// built script is not committed or a committed one is no longer built.
// Run with `npm run check:injected --workspace ow-tauri`; CI runs it on Linux.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const committedDir = join(root, '../../crates/tauri-plugin-overwolf/js');
const outDir = mkdtempSync(join(tmpdir(), 'ow-tauri-injected-'));
const rolldownCli = join(
  dirname(createRequire(import.meta.url).resolve('rolldown/package.json')),
  'bin/cli.mjs',
);

/** Script files in a directory (missing directory: none). */
function scripts(dir) {
  try {
    return readdirSync(dir)
      .filter((name) => name.endsWith('.js'))
      .sort();
  } catch {
    return [];
  }
}

/** Contents with line endings normalised (a Windows checkout may convert them). */
function read(file) {
  return readFileSync(file, 'utf8').replace(/\r\n/g, '\n');
}

const problems = [];
try {
  execFileSync(process.execPath, [rolldownCli, '-c', 'rolldown.config.mjs'], {
    cwd: root,
    stdio: ['ignore', 'ignore', 'inherit'],
    env: { ...process.env, OW_TAURI_INJECTED_OUT_DIR: outDir },
  });
  const built = scripts(outDir);
  const committed = scripts(committedDir);
  if (built.length === 0) problems.push('the build produced no scripts');
  for (const name of built) {
    if (!committed.includes(name)) problems.push(`js/${name} is built but not committed`);
    else if (read(join(outDir, name)) !== read(join(committedDir, name)))
      problems.push(`js/${name} differs from a fresh build`);
  }
  for (const name of committed)
    if (!built.includes(name)) problems.push(`js/${name} is committed but no longer built`);
} finally {
  rmSync(outDir, { recursive: true, force: true });
}

if (problems.length > 0) {
  console.error(
    `The injected scripts in crates/tauri-plugin-overwolf/js are stale:\n  ${problems.join('\n  ')}\n` +
      'Run `npm run build:injected --workspace ow-tauri` and commit crates/tauri-plugin-overwolf/js.',
  );
  process.exit(1);
}
console.log('crates/tauri-plugin-overwolf/js matches a fresh build.');
