// @vitest-environment node
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { capabilitiesOf, grants, init, parseLoose } from './init.js';
import type { Logger } from './sign.js';

function collectLog(): Logger & { lines: string[] } {
  const lines: string[] = [];
  return {
    lines,
    info: (m) => lines.push(`info: ${m}`),
    warn: (m) => lines.push(`warn: ${m}`),
  };
}

let dir: string;

function read(name: string): unknown {
  return JSON.parse(readFileSync(join(dir, name), 'utf8'));
}

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'ow-tauri-init-'));
  writeFileSync(
    join(dir, 'tauri.conf.json'),
    JSON.stringify(
      {
        productName: 'My Game App',
        identifier: 'com.example.mygameapp',
        app: { windows: [{ label: 'hud', title: 'HUD' }] },
      },
      null,
      2,
    ),
  );
});

afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
});

describe('ow-tauri init', () => {
  it('sets a fresh app up, and a second run changes nothing', async () => {
    const log = collectLog();
    expect((await init({ tauriDir: dir, author: 'Example Studio', log })).changed).toEqual([
      'tauri.conf.json',
      join('capabilities', 'default.json'),
      'tauri.windows.conf.json',
      '.gitignore',
    ]);
    expect(read('tauri.conf.json')).toEqual({
      productName: 'My Game App',
      identifier: 'com.example.mygameapp',
      app: { windows: [{ label: 'hud', title: 'HUD' }] },
      plugins: {
        overwolf: { author: 'Example Studio', name: 'My Game App', ads: { testAd: true } },
      },
    });
    expect(read(join('capabilities', 'default.json'))).toEqual({
      $schema: '../gen/schemas/desktop-schema.json',
      identifier: 'default',
      description: 'Main webview: core APIs and Overwolf ads, consent and identity',
      webviews: ['hud'],
      permissions: ['core:default', 'overwolf:default'],
    });
    expect(read('tauri.windows.conf.json')).toEqual({
      bundle: {
        targets: ['nsis'],
        windows: { nsis: { installerHooks: './gen/overwolf/installer-hooks.nsh' } },
      },
    });
    expect(readFileSync(join(dir, '.gitignore'), 'utf8')).toBe('/gen/overwolf\n');
    expect(log.lines.join('\n')).toContain('remove it before shipping');

    const again = collectLog();
    expect((await init({ tauriDir: dir, author: 'Example Studio', log: again })).changed).toEqual(
      [],
    );
    expect(again.lines.filter((l) => l.includes('up to date'))).toHaveLength(4);
  });

  it('merges into existing files', async () => {
    mkdirSync(join(dir, 'capabilities'));
    writeFileSync(
      join(dir, 'capabilities', 'default.json'),
      JSON.stringify({ identifier: 'default', windows: ['main'], permissions: ['core:default'] }),
    );
    writeFileSync(
      join(dir, 'tauri.windows.conf.json'),
      JSON.stringify({ bundle: { windows: { wix: {} } } }),
    );
    writeFileSync(join(dir, '.gitignore'), '/target');
    const log = collectLog();
    await init({ tauriDir: dir, author: 'A', name: 'Pinned Name', log });
    expect(read(join('capabilities', 'default.json'))).toEqual({
      identifier: 'default',
      windows: ['main'],
      permissions: ['core:default', 'overwolf:default'],
    });
    expect(log.lines.join('\n')).toContain('selects "windows"');
    expect(read('tauri.windows.conf.json')).toEqual({
      bundle: {
        windows: { wix: {}, nsis: { installerHooks: './gen/overwolf/installer-hooks.nsh' } },
      },
    });
    expect(readFileSync(join(dir, '.gitignore'), 'utf8')).toBe('/target\n/gen/overwolf\n');
    expect(read('tauri.conf.json')).toMatchObject({
      plugins: { overwolf: { author: 'A', name: 'Pinned Name' } },
    });
  });

  it('leaves custom NSIS hooks, other grants and covering .gitignore lines alone', async () => {
    mkdirSync(join(dir, 'capabilities'));
    writeFileSync(
      join(dir, 'capabilities', 'ads.json'),
      JSON.stringify([
        { identifier: 'ads', webviews: ['hud'], permissions: [{ identifier: 'overwolf:default' }] },
      ]),
    );
    writeFileSync(join(dir, 'capabilities', 'broken.json'), '{');
    writeFileSync(
      join(dir, 'tauri.windows.conf.json'),
      JSON.stringify({ bundle: { windows: { nsis: { installerHooks: './my-hooks.nsh' } } } }),
    );
    writeFileSync(join(dir, '.gitignore'), 'gen/\n');
    const log = collectLog();
    const result = await init({ tauriDir: dir, author: 'A', log });
    expect(result.changed).toEqual(['tauri.conf.json']);
    expect(log.lines.join('\n')).toContain('has its own NSIS hooks');
  });

  it('never changes an existing author or name, and needs both', async () => {
    const log = collectLog();
    await expect(init({ tauriDir: dir, log })).rejects.toThrow('pass --author');
    writeFileSync(
      join(dir, 'tauri.conf.json'),
      JSON.stringify({ plugins: { overwolf: { author: 'A', ads: {} } } }),
    );
    await expect(init({ tauriDir: dir, log })).rejects.toThrow('pass --name');
    await expect(init({ tauriDir: dir, author: 'B', name: 'N', log })).rejects.toThrow(
      'plugins.overwolf.author is already "A"',
    );
    await init({ tauriDir: dir, author: 'A', name: 'N', log });
    expect(read('tauri.conf.json')).toEqual({
      plugins: { overwolf: { author: 'A', ads: {}, name: 'N' } },
    });
  });
});

describe('capability helpers', () => {
  it('read every capability file shape', () => {
    expect(capabilitiesOf({ identifier: 'a' })).toEqual([{ identifier: 'a' }]);
    expect(capabilitiesOf([{ identifier: 'a' }, 1])).toEqual([{ identifier: 'a' }]);
    expect(capabilitiesOf({ capabilities: [{ identifier: 'b' }] })).toEqual([{ identifier: 'b' }]);
    expect(capabilitiesOf('x')).toEqual([]);
    expect(grants({ permissions: ['overwolf:default'] }, 'overwolf:default')).toBe(true);
    expect(grants({ permissions: [{ identifier: 'overwolf:default' }] }, 'overwolf:default')).toBe(
      true,
    );
    expect(grants({}, 'overwolf:default')).toBe(false);
    expect(parseLoose(undefined)).toBeUndefined();
    expect(parseLoose('{')).toBeUndefined();
    expect(parseLoose('\uFEFF[1]')).toEqual([1]);
  });
});
