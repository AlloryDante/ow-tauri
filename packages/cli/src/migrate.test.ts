// @vitest-environment node
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import type { JsonObject } from './json.js';
import { electronAuthor, migrate, migrateBlock } from './migrate.js';
import type { Logger } from './sign.js';
import { loadTauriConfig, resolveIdentity } from './tauri-config.js';

/** CONTRACT G.2 test vectors: `package.json` variants and the uid ow-electron runs with. */
const VECTORS: [number, JsonObject, string][] = [
  [
    1,
    { name: 'parity-harness', author: { name: 'Example Studio' } },
    'binaioonkjpolnojeenpbmjmbfkbmffcekndbmdk',
  ],
  [
    1,
    { name: 'parity-harness', author: 'Example Studio' },
    'binaioonkjpolnojeenpbmjmbfkbmffcekndbmdk',
  ],
  [
    1,
    {
      name: 'parity-harness',
      author: 'Example Studio',
      build: { productName: 'Parity Build Name' },
    },
    'binaioonkjpolnojeenpbmjmbfkbmffcekndbmdk',
  ],
  [
    2,
    { name: 'parity-harness', productName: 'Parity Harness', author: { name: 'Example Studio' } },
    'bijigndkghcikkfmhgkmicdkjpdehpjafgpmdhcc',
  ],
  [
    3,
    { name: 'parity-harness', author: 'Overwolf Ltd.' },
    'djaoacjhpjaenfddlfmkeoiklmccgcgcgeknhmgj',
  ],
  [
    4,
    { productName: 'Parity Harness', author: 'Overwolf Ltd.' },
    'aejkligdodglhcjinbhdcnlohocenfkpdihjacdg',
  ],
  [
    5,
    { name: 'parity-harness', author: 'Example Studio <dev@example.com> (https://example.com)' },
    'agmekflfehlhfcnofnhghbgohnigngnkddpkdbnc',
  ],
  [
    6,
    {
      name: 'parity-harness',
      productName: 'Parity Harness',
      author: 'Example Studio <dev@example.com> (https://example.com)',
    },
    'cmbaaahkhdbkbbfcmenfbmmngmommpjjacllbgan',
  ],
  [
    7,
    { name: 'parity-harness', author: 'Example Studio <dev@example.com>' },
    'mcfopdapolegaeddgbbfedginnmcnmjldgdcbcjo',
  ],
  [8, { name: 'parity-harness' }, 'nbhlaphlggihmjefpjdelbobckfhklbfkiicjaja'],
  [8, { name: 'parity-harness', author: {} }, 'nbhlaphlggihmjefpjdelbobckfhklbfkiicjaja'],
  [8, { name: 'parity-harness', author: '' }, 'nbhlaphlggihmjefpjdelbobckfhklbfkiicjaja'],
  [9, { productName: 'Parity Harness' }, 'fifpcfmoobnjlimjhefehejankadpajlfgbmpheo'],
  [9, { productName: 'Parity Harness', author: {} }, 'fifpcfmoobnjlimjhefehejankadpajlfgbmpheo'],
  [9, { productName: 'Parity Harness', author: '' }, 'fifpcfmoobnjlimjhefehejankadpajlfgbmpheo'],
  [
    10,
    { productName: 'Pârity Ünicode', author: { name: 'Exämple' } },
    'mmfoflmmchoacblhjlimpanaijdnhgoalaloihjd',
  ],
  [
    11,
    { productName: "O'Brien Tools", author: { name: "D'Arcy" } },
    'khalfglcmeemfnjoldckbfmeeidgkoabeebkpbbl',
  ],
  [
    12,
    { productName: ' Parity Harness ', author: { name: ' Example Studio ' } },
    'cppaiialckdbmhdojecejpjafcblbingfdiffkdi',
  ],
  [
    13,
    {
      name: 'any-name',
      author: 'Any Author',
      overwolf: { uid: 'aaaabbbbccccddddeeeeffffgggghhhhiiiijjjj' },
    },
    'aaaabbbbccccddddeeeeffffgggghhhhiiiijjjj',
  ],
];

function collectLog(): Logger & { lines: string[] } {
  const lines: string[] = [];
  return {
    lines,
    info: (m) => lines.push(`info: ${m}`),
    warn: (m) => lines.push(`warn: ${m}`),
  };
}

describe('migrateBlock', () => {
  it.each(VECTORS)('reproduces uid vector %i (%j)', async (_n, pkg, uid) => {
    const migration = migrateBlock(pkg);
    expect(migration.uid).toBe(uid);
    // Always both identity inputs (DX-M7).
    expect(typeof migration.block['author']).toBe('string');
    expect(typeof migration.block['name']).toBe('string');
    // The block alone, as plugins.overwolf of a config with another
    // productName, makes the plugin resolve the same uid.
    const config = { productName: 'Tauri Bundle Name', plugins: { overwolf: migration.block } };
    const identity = await resolveIdentity(config, tmpdir());
    expect(identity.uid).toBe(uid);
    expect(identity.pinned).toBe(true);
  });

  it('reads the identity from the packaged form (extraMetadata)', () => {
    const migration = migrateBlock({
      name: 'parity-harness',
      author: 'Someone Else',
      build: { extraMetadata: { author: 'Overwolf Ltd.' } },
    });
    expect(migration.block['author']).toBe('Overwolf Ltd.');
    expect(migration.uid).toBe('djaoacjhpjaenfddlfmkeoiklmccgcgcgeknhmgj');
  });

  it('writes the Overwolf flags of build.overwolf', () => {
    expect(migrateBlock({ name: 'a', author: 'b' }).block).toEqual({
      author: 'b',
      name: 'a',
      ads: { disableOptimization: false },
      signing: { requireSigning: true, owCertSigning: false },
    });
    expect(
      migrateBlock({
        name: 'a',
        author: 'b',
        build: {
          overwolf: {
            disableAdOptimization: true,
            requireSigning: false,
            enableOWCertSigning: true,
          },
        },
      }).block,
    ).toMatchObject({
      ads: { disableOptimization: true },
      signing: { requireSigning: false, owCertSigning: true },
    });
  });

  it('warns about defaults, invalid uids and packages', () => {
    const noAuthor = migrateBlock({ name: 'a' });
    expect(noAuthor.block['author']).toBe('unknown');
    expect(noAuthor.warnings.join('\n')).toContain('has no author');
    expect(migrateBlock({ name: 'a', author: 'unknown' }).warnings).toEqual([]);
    const badUid = migrateBlock({ name: 'a', author: 'b', overwolf: { uid: 'not/a uid' } });
    expect(badUid.block).not.toHaveProperty('uid');
    expect(badUid.uid).toBe(badUid.cuid);
    expect(badUid.warnings.join('\n')).toContain('ow-electron ignores it');
    expect(
      migrateBlock({ name: 'a', author: 'b', overwolf: { uid: '  ' } }).block,
    ).not.toHaveProperty('uid');
    expect(migrateBlock({ name: 'a', author: 'b', overwolf: { uid: ' abc ' } }).block['uid']).toBe(
      'abc',
    );
    const packages = migrateBlock({
      name: 'a',
      author: 'b',
      overwolf: { packages: ['gep', 'overlay'] },
    });
    expect(packages.warnings).toEqual([
      'overwolf.packages (gep, overlay): packages are not available on Tauri yet',
    ]);
    expect(() => migrateBlock({ version: '1.0.0' })).toThrow(
      'the package.json has neither "productName" nor "name"',
    );
  });

  it('reads the author like ow-electron', () => {
    expect(electronAuthor('A <a@example.com>')).toBe('A <a@example.com>');
    expect(electronAuthor({ name: 'A', email: 'a@example.com' })).toBe('A');
    for (const none of [undefined, null, '', {}, { name: '' }, { email: 'x' }, 7]) {
      expect(electronAuthor(none)).toBe('unknown');
    }
  });
});

describe('ow-tauri migrate', () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'ow-tauri-migrate-'));
    mkdirSync(join(dir, 'electron'));
    mkdirSync(join(dir, 'src-tauri'));
    writeFileSync(
      join(dir, 'electron', 'package.json'),
      JSON.stringify({
        name: 'parity-harness',
        productName: 'Parity Harness',
        author: { name: 'Example Studio' },
        build: { overwolf: { disableAdOptimization: true } },
      }),
    );
    writeFileSync(
      join(dir, 'src-tauri', 'tauri.conf.json'),
      '{\n    "productName": "New Name",\n    "plugins": {\n        "overwolf": { "ads": { "testAd": true } }\n    }\n}\n',
    );
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it('prints the block, and merges it with --write keeping other keys and the indentation', async () => {
    const out: string[] = [];
    const log = collectLog();
    await migrate({
      from: 'electron/package.json',
      write: 'src-tauri/tauri.conf.json',
      cwd: dir,
      log,
      out: (t) => out.push(t),
    });
    expect(JSON.parse(out.join(''))).toEqual({
      plugins: {
        overwolf: {
          author: 'Example Studio',
          name: 'Parity Harness',
          ads: { disableOptimization: true },
          signing: { requireSigning: true, owCertSigning: false },
        },
      },
    });
    const text = readFileSync(join(dir, 'src-tauri', 'tauri.conf.json'), 'utf8');
    expect(text.startsWith('{\n    "productName": "New Name",')).toBe(true);
    expect(JSON.parse(text)).toEqual({
      productName: 'New Name',
      plugins: {
        overwolf: {
          ads: { testAd: true, disableOptimization: true },
          author: 'Example Studio',
          name: 'Parity Harness',
          signing: { requireSigning: true, owCertSigning: false },
        },
      },
    });
    const loaded = await loadTauriConfig({
      tauriDir: join(dir, 'src-tauri'),
      target: 'windows',
      env: {},
      cwd: dir,
    });
    expect((await resolveIdentity(loaded.config, loaded.tauriDir)).uid).toBe(
      'bijigndkghcikkfmhgkmicdkjpdehpjafgpmdhcc',
    );
    expect(log.lines.join('\n')).toContain('uid bijigndkghcikkfmhgkmicdkjpdehpjafgpmdhcc');
  });

  it('warns when tauri.conf.json already pins another uid', async () => {
    writeFileSync(
      join(dir, 'src-tauri', 'tauri.conf.json'),
      JSON.stringify({ plugins: { overwolf: { uid: 'abc' } } }),
    );
    const log = collectLog();
    await migrate({
      from: 'electron/package.json',
      write: 'src-tauri/tauri.conf.json',
      cwd: dir,
      log,
      out: () => undefined,
    });
    expect(log.lines.join('\n')).toContain('pins plugins.overwolf.uid "abc"');
  });

  it('fails on a missing target file', async () => {
    await expect(
      migrate({
        from: 'electron/package.json',
        write: 'missing.json',
        cwd: dir,
        log: collectLog(),
        out: () => undefined,
      }),
    ).rejects.toThrow('[OW] cannot read');
  });
});
