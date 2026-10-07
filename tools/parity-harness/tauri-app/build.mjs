// Builds the Tauri edition of the parity harness app: the web assets into
// dist/ (bundled with the workspace's rolldown and ow-tauri), then the Rust
// shell in debug. `run.mjs --host tauri` calls buildTauriApp() once per run.
//
//   node tauri-app/build.mjs          # build and print the binary path

import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, '..', '..', '..');
const distDir = join(here, 'dist');
const crateDir = join(here, 'src-tauri');

/** Bundles web/main.js (ow-tauri/electron inlined) and copies the pages. */
export async function buildWeb() {
  const require = createRequire(join(repoRoot, 'package.json'));
  // A file URL: on Windows a bare absolute path reads as a URL scheme (`d:`).
  const { build } = await import(pathToFileURL(require.resolve('rolldown')).href);
  mkdirSync(distDir, { recursive: true });
  await build({
    input: join(here, 'web', 'main.js'),
    platform: 'browser',
    resolve: {
      alias: { electron: 'ow-tauri/electron' },
      modules: [join(repoRoot, 'node_modules'), 'node_modules'],
    },
    output: { file: join(distDir, 'main.js'), format: 'esm' },
    logLevel: 'warn',
  });
  for (const file of ['main.html', 'index.html', 'page-shim.js']) {
    copyFileSync(join(here, 'web', file), join(distDir, file));
  }
  // The harness page itself is shared with the ow-electron app.
  copyFileSync(join(here, '..', 'app', 'page.js'), join(distDir, 'page.js'));
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
