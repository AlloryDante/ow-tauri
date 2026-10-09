// node --test scripts/release/*.test.mjs
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { after, describe, it } from 'node:test';

import { check, distTag, isValidVersion, REPO_ROOT, set } from './version-sync.mjs';

const dirs = [];
after(() => dirs.forEach((d) => rmSync(d, { recursive: true, force: true })));

const json = (v) => `${JSON.stringify(v, null, 2)}\n`;

/** A minimal checkout with every kind of place that carries the version. */
function fixture(v = '0.1.0') {
  const root = mkdtempSync(join(tmpdir(), 'version-sync-'));
  dirs.push(root);
  const files = {
    'package.json': json({ name: 'ws', private: true, workspaces: ['packages/*', 'examples/app'] }),
    'Cargo.toml': [
      '[workspace]',
      'members = ["crates/tauri-plugin-overwolf"]',
      '',
      '[workspace.package]',
      `version = "${v}"`,
      'edition = "2024"',
      '',
      '[workspace.dependencies]',
      'serde = { version = "1" }',
      '',
    ].join('\n'),
    'crates/tauri-plugin-overwolf/Cargo.toml': [
      '[package]',
      'name = "tauri-plugin-overwolf"',
      'version.workspace = true',
      '',
      '[dependencies]',
      'semver = { version = "1" }',
      '',
      "[target.'cfg(windows)'.dependencies]",
      `tauri-plugin-overwolf-unstable = { path = "../tauri-plugin-overwolf-unstable", version = "${v}", optional = true }`,
      '',
    ].join('\n'),
    'packages/api/src/internal.ts': `/** doc */\nexport const RUNTIME_VERSION = '${v}';\n`,
    'packages/api/package.json': json({
      name: 'tauri-plugin-overwolf-api',
      version: v,
      dependencies: { '@tauri-apps/api': '^2.12.1' },
      devDependencies: { nested: { version: '9.9.9' } },
    }),
    'packages/cli/package.json': json({ name: 'tauri-plugin-overwolf-cli', version: v }),
    'packages/guest-shims/package.json': json({
      name: 'tauri-plugin-overwolf-guest-shims',
      version: v,
      private: true,
    }),
    'examples/app/package.json': json({
      name: 'app',
      version: '1.0.0',
      private: true,
      dependencies: { 'tauri-plugin-overwolf-api': `^${v}` },
      devDependencies: { 'tauri-plugin-overwolf-cli': `~${v}` },
    }),
    'package-lock.json': json({
      name: 'ws',
      lockfileVersion: 3,
      packages: {
        '': { name: 'ws', workspaces: ['packages/*', 'examples/app'] },
        'examples/app': {
          version: '1.0.0',
          dependencies: { 'tauri-plugin-overwolf-api': `^${v}` },
          devDependencies: { 'tauri-plugin-overwolf-cli': `~${v}` },
        },
        'node_modules/tauri-plugin-overwolf-api': { resolved: 'packages/api', link: true },
        'node_modules/left-pad': { version: v },
        'packages/api': { name: 'tauri-plugin-overwolf-api', version: v },
        'packages/cli': { name: 'tauri-plugin-overwolf-cli', version: v },
        'packages/guest-shims': { name: 'tauri-plugin-overwolf-guest-shims', version: v },
      },
    }),
    'Cargo.lock': [
      'version = 4',
      '',
      '[[package]]',
      'name = "ow-tauri-acl-tests"',
      `version = "${v}"`,
      'dependencies = [',
      ' "tauri-plugin-overwolf",',
      ']',
      '',
      '[[package]]',
      'name = "semver"',
      `version = "${v}"`,
      'source = "registry+https://github.com/rust-lang/crates.io-index"',
      'checksum = "00"',
      '',
      '[[package]]',
      'name = "tauri-plugin-overwolf"',
      `version = "${v}"`,
      'dependencies = [',
      ' "tauri-plugin-overwolf-unstable",',
      ']',
      '',
      '[[package]]',
      'name = "tauri-plugin-overwolf-unstable"',
      `version = "${v}"`,
      '',
    ].join('\n'),
    'examples/app/src-tauri/Cargo.lock': [
      'version = 4',
      '',
      '[[package]]',
      'name = "tauri-plugin-overwolf"',
      `version = "${v}"`,
      '',
      '[[package]]',
      'name = "tauri-plugin-overwolf"',
      'version = "0.0.9"',
      'source = "registry+https://github.com/rust-lang/crates.io-index"',
      '',
    ].join('\n'),
  };
  for (const [file, text] of Object.entries(files)) {
    mkdirSync(dirname(join(root, file)), { recursive: true });
    writeFileSync(join(root, file), text);
  }
  return root;
}

const read = (root, file) => readFileSync(join(root, file), 'utf8');

describe('isValidVersion / distTag', () => {
  it('accepts SemVer release versions', () => {
    for (const v of ['0.1.0', '1.0.0', '1.0.0-rc.1', '1.0.0-alpha.0.beta', '10.20.30']) {
      assert.equal(isValidVersion(v), true, v);
    }
  });
  it('refuses a leading v, build metadata and malformed versions', () => {
    for (const v of [
      'v1.0.0',
      '1.0',
      '01.0.0',
      '1.0.0-',
      '1.0.0+build.1',
      '1.0.0-rc.01',
      '',
      ' 1.0.0',
    ]) {
      assert.equal(isValidVersion(v), false, v);
    }
  });
  it('uses next for a prerelease and latest otherwise', () => {
    assert.equal(distTag('1.0.0-rc.1'), 'next');
    assert.equal(distTag('1.0.0'), 'latest');
    assert.throws(() => distTag('v1.0.0'));
  });
});

describe('check', () => {
  it('passes when every place carries one version', () => {
    const root = fixture();
    const r = check(root);
    assert.deepEqual(r.problems, []);
    assert.equal(r.version, '0.1.0');
    // workspace, unstable pin, RUNTIME_VERSION, 3 package.json, 2 example
    // ranges, package-lock.json, 2 Cargo.lock files
    assert.equal(r.rows.length, 11);
  });
  it('names the file that drifted', () => {
    const root = fixture();
    writeFileSync(
      join(root, 'packages/cli/package.json'),
      read(root, 'packages/cli/package.json').replace('0.1.0', '0.2.0'),
    );
    const r = check(root);
    assert.equal(r.ok, false);
    assert.match(r.problems.join('\n'), /packages\/cli\/package\.json \(version\): 0\.2\.0/);
  });
  it('compares against --expect', () => {
    const root = fixture();
    assert.equal(check(root, '0.1.0').ok, true);
    const r = check(root, '1.0.0-rc.1');
    assert.equal(r.ok, false);
    assert.equal(r.problems.length, r.rows.length);
  });
  it('reports a missing place instead of skipping it', () => {
    const root = fixture();
    writeFileSync(join(root, 'packages/api/src/internal.ts'), '// gone\n');
    const r = check(root);
    assert.equal(r.ok, false);
    assert.match(r.problems.join('\n'), /internal\.ts \(RUNTIME_VERSION\): no RUNTIME_VERSION/);
  });
  it('passes on this repository', () => {
    const r = check(REPO_ROOT);
    assert.deepEqual(r.problems, []);
  });
});

describe('set', () => {
  it('bumps every place and nothing else', () => {
    const root = fixture();
    const before = Object.fromEntries(
      ['Cargo.lock', 'examples/app/src-tauri/Cargo.lock', 'package-lock.json'].map((f) => [
        f,
        read(root, f),
      ]),
    );
    const changed = set(root, '1.0.0-rc.1');
    // 11 places in 10 files (the example carries two ranges).
    assert.equal(changed.length, 10);
    assert.deepEqual(check(root, '1.0.0-rc.1').problems, []);

    // Range operators are kept.
    const app = JSON.parse(read(root, 'examples/app/package.json'));
    assert.equal(app.dependencies['tauri-plugin-overwolf-api'], '^1.0.0-rc.1');
    assert.equal(app.devDependencies['tauri-plugin-overwolf-cli'], '~1.0.0-rc.1');
    assert.equal(app.version, '1.0.0');

    // Nested "version" keys and registry packages keep their versions.
    const api = JSON.parse(read(root, 'packages/api/package.json'));
    assert.equal(api.devDependencies.nested.version, '9.9.9');
    const lock = JSON.parse(read(root, 'package-lock.json'));
    assert.equal(lock.packages['node_modules/left-pad'].version, '0.1.0');
    assert.equal(lock.packages['packages/api'].version, '1.0.0-rc.1');
    assert.match(read(root, 'Cargo.lock'), /name = "semver"\nversion = "0\.1\.0"\nsource/);
    assert.match(
      read(root, 'examples/app/src-tauri/Cargo.lock'),
      /name = "tauri-plugin-overwolf"\nversion = "0\.0\.9"\nsource/,
    );

    // Only version strings changed: same line count, same lines elsewhere.
    for (const [file, text] of Object.entries(before)) {
      const a = text.split('\n');
      const b = read(root, file).split('\n');
      assert.equal(a.length, b.length, file);
      const diff = a.filter((line, i) => line !== b[i]);
      assert.ok(
        diff.every((line) => line.includes('0.1.0')),
        `${file}: ${diff.join(' | ')}`,
      );
    }

    // The workspace's other tables are untouched.
    assert.match(read(root, 'Cargo.toml'), /serde = \{ version = "1" \}/);
    assert.match(
      read(root, 'crates/tauri-plugin-overwolf/Cargo.toml'),
      /semver = \{ version = "1" \}/,
    );
  });
  it('is idempotent', () => {
    const root = fixture();
    set(root, '1.0.0');
    assert.deepEqual(set(root, '1.0.0'), []);
  });
  it('refuses an invalid version and writes nothing', () => {
    const root = fixture();
    const text = read(root, 'Cargo.toml');
    assert.throws(() => set(root, 'v1.0.0'), /not a release version/);
    assert.equal(read(root, 'Cargo.toml'), text);
  });
});
