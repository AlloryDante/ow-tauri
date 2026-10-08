// @vitest-environment node
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { detectIndent, formatJson, mergePatch, parseJsonObject, readJsonObject } from './json.js';
import {
  cargoPackage,
  computeUid,
  findTauriDir,
  loadTauriConfig,
  normalizeUid,
  resolveIdentity,
  targetOf,
  type TauriTarget,
} from './tauri-config.js';

const FIXTURES = fileURLToPath(new URL('../../../fixtures/config-merge/', import.meta.url));

describe('mergePatch (RFC 7396)', () => {
  it('follows the examples of RFC 7396 appendix A', () => {
    const cases: [unknown, unknown, unknown][] = [
      [{ a: 'b' }, { a: 'c' }, { a: 'c' }],
      [{ a: 'b' }, { b: 'c' }, { a: 'b', b: 'c' }],
      [{ a: 'b' }, { a: null }, {}],
      [{ a: 'b', b: 'c' }, { a: null }, { b: 'c' }],
      [{ a: ['b'] }, { a: 'c' }, { a: 'c' }],
      [{ a: 'c' }, { a: ['b'] }, { a: ['b'] }],
      [{ a: { b: 'c' } }, { a: { b: 'd', c: null } }, { a: { b: 'd' } }],
      [{ a: [{ b: 'c' }] }, { a: [1] }, { a: [1] }],
      [
        ['a', 'b'],
        ['c', 'd'],
        ['c', 'd'],
      ],
      [{ a: 'b' }, ['c'], ['c']],
      [{ a: 'foo' }, null, null],
      [{ a: 'foo' }, 'bar', 'bar'],
      [{ e: null }, { a: 1 }, { e: null, a: 1 }],
      [[1, 2], { a: 'b', c: null }, { a: 'b' }],
      [{}, { a: { bb: { ccc: null } } }, { a: { bb: {} } }],
    ];
    for (const [target, patch, result] of cases) expect(mergePatch(target, patch)).toEqual(result);
  });

  it('never changes its inputs', () => {
    const target = { a: { b: 1 } };
    mergePatch(target, { a: { b: 2 } });
    expect(target).toEqual({ a: { b: 1 } });
  });
});

describe('JSON helpers', () => {
  it('keeps the indentation of an existing file', () => {
    expect(detectIndent('{\n    "a": 1\n}')).toBe(4);
    expect(detectIndent('{\n\t"a": 1\n}')).toBe('\t');
    expect(detectIndent('{"a":1}')).toBe(2);
    expect(formatJson({ a: 1 }, '{\n\t"a": 0\n}\n')).toBe('{\n\t"a": 1\n}\n');
    expect(formatJson({ a: 1 })).toBe('{\n  "a": 1\n}\n');
  });

  it('names the file of a bad or missing document', async () => {
    expect(parseJsonObject('\uFEFF{"a":1}', 'x')).toEqual({ a: 1 });
    expect(() => parseJsonObject('{', 'x.json')).toThrow('[OW] x.json is not valid JSON');
    expect(() => parseJsonObject('[]', 'x.json')).toThrow('[OW] x.json is not a JSON object');
    await expect(readJsonObject('/nonexistent/ow-tauri/x.json')).rejects.toThrow(
      '[OW] cannot read /nonexistent/ow-tauri/x.json',
    );
  });
});

describe('targetOf', () => {
  it('maps Node platforms and Rust triples as Target::from_triple does', () => {
    expect(['win32', 'x86_64-pc-windows-msvc', 'windows'].map(targetOf)).toEqual([
      'windows',
      'windows',
      'windows',
    ]);
    expect(['darwin', 'aarch64-apple-darwin', 'macos'].map(targetOf)).toEqual([
      'macos',
      'macos',
      'macos',
    ]);
    expect(['linux', 'x86_64-unknown-linux-gnu', 'freebsd'].map(targetOf)).toEqual([
      'linux',
      'linux',
      'linux',
    ]);
    expect(['aarch64-linux-android', 'android'].map(targetOf)).toEqual(['android', 'android']);
    expect(['aarch64-apple-ios', 'ios'].map(targetOf)).toEqual(['ios', 'ios']);
  });
});

describe('the uid formula (CONTRACT G.2)', () => {
  it('reproduces the test vectors', () => {
    expect(computeUid('Example Studio', 'parity-harness')).toBe(
      'binaioonkjpolnojeenpbmjmbfkbmffcekndbmdk',
    );
    expect(computeUid('unknown', 'parity-harness')).toBe(
      'nbhlaphlggihmjefpjdelbobckfhklbfkiicjaja',
    );
    expect(computeUid("D'Arcy", "O'Brien Tools")).toBe('khalfglcmeemfnjoldckbfmeeidgkoabeebkpbbl');
  });

  it('accepts 1 to 64 ASCII letters or digits after trimming', () => {
    expect(normalizeUid(' abc123 ')).toBe('abc123');
    expect(normalizeUid('a'.repeat(64))).toBe('a'.repeat(64));
    for (const bad of ['', '   ', 'a'.repeat(65), 'a/b', '../x', 'ä', 'a b']) {
      expect(normalizeUid(bad)).toBeUndefined();
    }
  });

  it('reads the Cargo package name and version', () => {
    expect(
      cargoPackage(
        '[workspace]\nname = "w"\n[package]\nname = "app" # x\nversion = \'1.2.3\'\n[dependencies]\nname = "d"\n',
      ),
    ).toEqual({ name: 'app', version: '1.2.3' });
    expect(cargoPackage('')).toEqual({});
  });
});

/** One fixture case of fixtures/config-merge. */
interface Case {
  target: TauriTarget;
  tauriConfig?: string;
}

interface Expected {
  config: Record<string, unknown>;
  identity: { uid: string; cuid: string; name: string; author: string; version: string };
}

describe('config-merge fixtures (shared with build::run)', () => {
  const cases = readdirSync(FIXTURES, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();

  it('cover base, overlays and TAURI_CONFIG', () => {
    expect(cases.length).toBeGreaterThanOrEqual(8);
    const all = cases.map(
      (name) => JSON.parse(readFileSync(join(FIXTURES, name, 'case.json'), 'utf8')) as Case,
    );
    expect(all.some((c) => c.tauriConfig !== undefined)).toBe(true);
    expect(new Set(all.map((c) => c.target))).toEqual(new Set(['windows', 'macos', 'linux']));
    expect(
      cases.filter((name) =>
        readdirSync(join(FIXTURES, name)).some((f) => /^tauri\.[a-z]+\.conf\.json$/.test(f)),
      ).length,
    ).toBeGreaterThanOrEqual(3);
  });

  it.each(cases)('%s', async (name) => {
    const dir = join(FIXTURES, name);
    const spec = JSON.parse(readFileSync(join(dir, 'case.json'), 'utf8')) as Case;
    const expected = JSON.parse(readFileSync(join(dir, 'expected.json'), 'utf8')) as Expected;
    const env = spec.tauriConfig === undefined ? {} : { TAURI_CONFIG: spec.tauriConfig };
    const loaded = await loadTauriConfig({ tauriDir: dir, target: spec.target, env, cwd: dir });
    expect(loaded.config).toEqual(expected.config);
    const identity = await resolveIdentity(loaded.config, dir);
    expect({
      uid: identity.uid,
      cuid: identity.cuid,
      name: identity.name,
      author: identity.author,
      version: identity.version,
    }).toEqual(expected.identity);
  });
});

describe('loadTauriConfig and resolveIdentity', () => {
  let dir: string;
  let tauriDir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'ow-tauri-config-'));
    tauriDir = join(dir, 'src-tauri');
    mkdirSync(tauriDir);
    writeFileSync(
      join(tauriDir, 'tauri.conf.json'),
      JSON.stringify({ productName: 'Base', plugins: { overwolf: { author: 'A' } } }),
    );
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it('finds the Tauri folder from the project or the folder itself', () => {
    expect(findTauriDir(dir)).toBe(tauriDir);
    expect(findTauriDir(tauriDir)).toBe(tauriDir);
    expect(findTauriDir(dir, 'src-tauri')).toBe(tauriDir);
    expect(() => findTauriDir(join(dir, 'nowhere'))).toThrow(/no tauri\.conf\.json in/);
    mkdirSync(join(dir, 'toml'));
    writeFileSync(join(dir, 'toml', 'Tauri.toml'), '');
    expect(() => findTauriDir(dir, 'toml')).toThrow(/reads tauri\.conf\.json only/);
  });

  it('merges --config values in order into one patch that replaces TAURI_CONFIG', async () => {
    writeFileSync(
      join(dir, 'extra.json'),
      JSON.stringify({ plugins: { overwolf: { name: 'File' } } }),
    );
    const loaded = await loadTauriConfig({
      tauriDir,
      target: 'windows',
      env: { TAURI_CONFIG: '{"productName":"Env"}' },
      configs: ['{"plugins":{"overwolf":{"name":"Inline","author":null}}}', 'extra.json'],
      cwd: dir,
    });
    // As in the Tauri CLI, the values are merged into one patch first, so a
    // `null` in a --config value removes the key from that patch only.
    expect(loaded.extra).toEqual({ plugins: { overwolf: { name: 'File' } } });
    expect(loaded.config).toEqual({
      productName: 'Base',
      plugins: { overwolf: { author: 'A', name: 'File' } },
    });
    const env = await loadTauriConfig({
      tauriDir,
      target: 'windows',
      env: { TAURI_CONFIG: '{"productName":"Env"}' },
      cwd: dir,
    });
    expect(env.config['productName']).toBe('Env');
    expect(env.files).toEqual([join(tauriDir, 'tauri.conf.json')]);
    await expect(
      loadTauriConfig({ tauriDir, target: 'linux', env: { TAURI_CONFIG: 'x' }, cwd: dir }),
    ).rejects.toThrow('[OW] TAURI_CONFIG is not valid JSON');
  });

  it('refuses a JSON5 or TOML overlay instead of ignoring it', async () => {
    writeFileSync(join(tauriDir, 'tauri.windows.conf.json5'), '{}');
    await expect(
      loadTauriConfig({ tauriDir, target: 'windows', env: {}, cwd: dir }),
    ).rejects.toThrow(/reads JSON overlays only/);
  });

  it('reports what fell back to a default, and what pins the uid', async () => {
    const identity = await resolveIdentity({ productName: 'P' }, tauriDir);
    expect(identity).toMatchObject({
      author: 'unknown',
      name: 'P',
      nameSource: 'productName',
      pinned: false,
      uidConfigured: false,
      version: undefined,
    });
    expect(identity.warnings.join('\n')).toContain('plugins.overwolf.author is not set');
    expect(
      (await resolveIdentity({ plugins: { overwolf: { author: 'A', name: 'N' } } }, tauriDir))
        .pinned,
    ).toBe(true);
    expect(
      (await resolveIdentity({ productName: 'P', plugins: { overwolf: { uid: 'abc' } } }, tauriDir))
        .pinned,
    ).toBe(true);
    await expect(resolveIdentity({}, tauriDir)).rejects.toThrow(
      'set plugins.overwolf.name or productName',
    );
    for (const uid of [' ', 'a/b', 7]) {
      await expect(
        resolveIdentity({ productName: 'P', plugins: { overwolf: { uid } } }, tauriDir),
      ).rejects.toThrow('plugins.overwolf.uid: must be 1 to 64 ASCII letters or digits');
    }
    // `null` means unset, as in serde.
    expect(
      (await resolveIdentity({ productName: 'P', plugins: { overwolf: { uid: null } } }, tauriDir))
        .uidConfigured,
    ).toBe(false);
  });

  it('reads version files relative to the Tauri folder', async () => {
    writeFileSync(join(dir, 'package.json'), JSON.stringify({ version: '3.0.0' }));
    expect(
      (await resolveIdentity({ productName: 'P', version: '../package.json' }, tauriDir)).version,
    ).toBe('3.0.0');
    writeFileSync(join(dir, 'package.json'), JSON.stringify({ version: 3 }));
    await expect(
      resolveIdentity({ productName: 'P', version: '../package.json' }, tauriDir),
    ).rejects.toThrow(/"version" must be a string/);
    expect((await resolveIdentity({ productName: 'P', version: '1.0.0' }, tauriDir)).version).toBe(
      '1.0.0',
    );
  });
});
